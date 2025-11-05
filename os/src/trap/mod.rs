//! Trap handling functionality
//!
//! For rCore, we have a single trap entry point, namely `__alltraps`. At
//! initialization in [`init()`], we set the `stvec` CSR to point to it.
//!
//! All traps go through `__alltraps`, which is defined in `trap.S`. The
//! assembly language code does just enough work restore the kernel space
//! context, ensuring that Rust code safely runs, and transfers control to
//! [`trap_handler()`].
//!
//! It then calls different functionality based on what exactly the exception
//! was. For example, timer interrupts trigger task preemption, and syscalls go
//! to [`syscall()`].

mod context;

use crate::config::{TRAMPOLINE, TRAP_CONTEXT_BASE};
use crate::syscall::syscall;
use crate::task::{
    current_trap_cx, current_user_token, exit_current_and_run_next, suspend_current_and_run_next,
};
use crate::timer::set_next_trigger;
use core::arch::{asm, global_asm};
use riscv::register::{
    mtvec::TrapMode,
    scause::{self, Exception, Interrupt, Trap},
    sie, stval, stvec,
};

global_asm!(include_str!("trap.S"));

/// Initialize trap handling
pub fn init() {
    set_kernel_trap_entry();
}

fn set_kernel_trap_entry() {
    unsafe {
        stvec::write(trap_from_kernel as usize, TrapMode::Direct);
    }
}

//
/// 注：我们把 stvec 设置为内核和应用地址空间共享的跳板页面的起始地址 TRAMPOLINE
/// 而不是编译器在链接时看到的 __alltraps 的地址。
/// 这是因为启用分页模式之后，内核只能通过跳板页面上的虚拟地址来实际取得 __alltraps 和 __restore 的汇编代码。
fn set_user_trap_entry() {
    unsafe {
        stvec::write(TRAMPOLINE as usize, TrapMode::Direct);
    }
}

/// enable timer interrupt in supervisor mode
pub fn enable_timer_interrupt() {
    unsafe {
        sie::set_stimer();
    }
}


// 由于应用的 Trap 上下文不在内核地址空间，
// 因此我们调用 current_trap_cx 来获取当前应用的 Trap 上下文的可变引用而不是像之前那样作为参数传入 trap_handler 。
// 至于 Trap 处理的过程则没有发生什么变化。

// 注意到，在 trap_handler 的开头还调用 set_kernel_trap_entry 将 stvec 修改为同模块下另一个函数 trap_from_kernel 的地址。
// 这就是说，一旦进入内核后再次触发到 S态 Trap，则硬件在设置一些 CSR 寄存器之后，
// 会跳过对通用寄存器的保存过程，直接跳转到 trap_from_kernel 函数，在这里直接 panic 退出。
// 这是因为内核和应用的地址空间分离之后，U态 –> S态 与 S态 –> S态 的 Trap 上下文保存与恢复实现方式/Trap 处理逻辑有很大差别。
// 这里为了简单起见，弱化了 S态 –> S态的 Trap 处理过程：直接 panic 。

// 在 trap_handler 完成 Trap 处理之后，我们需要调用 trap_return 返回用户态：
/// trap handler
#[no_mangle]
pub fn trap_handler() -> ! {
    set_kernel_trap_entry();
    let cx: &mut TrapContext = current_trap_cx();
    let scause = scause::read(); // get trap cause
    let stval = stval::read(); // get extra value
    // trace!("into {:?}", scause.cause());
    match scause.cause() {
        Trap::Exception(Exception::UserEnvCall) => {
            // jump to next instruction anyway
            cx.sepc += 4;
            // get system call return value
            cx.x[10] = syscall(cx.x[17], [cx.x[10], cx.x[11], cx.x[12]]) as usize;
        }
        Trap::Exception(Exception::StoreFault)
        | Trap::Exception(Exception::StorePageFault)
        | Trap::Exception(Exception::LoadFault)
        | Trap::Exception(Exception::LoadPageFault) => {
            println!("[kernel] PageFault in application, bad addr = {:#x}, bad instruction = {:#x}, kernel killed it.", stval, cx.sepc);
            exit_current_and_run_next();
        }
        Trap::Exception(Exception::IllegalInstruction) => {
            println!("[kernel] IllegalInstruction in application, kernel killed it.");
            exit_current_and_run_next();
        }
        Trap::Interrupt(Interrupt::SupervisorTimer) => {
            set_next_trigger();
            suspend_current_and_run_next();
        }
        _ => {
            panic!(
                "Unsupported trap {:?}, stval = {:#x}!",
                scause.cause(),
                stval
            );
        }
    }
    //println!("before trap_return");
    trap_return();
}

#[no_mangle]
/// return to user space
/// set the new addr of __restore asm function in TRAMPOLINE page,
/// set the reg a0 = trap_cx_ptr, reg a1 = phy addr of usr page table,
/// finally, jump to new addr of __restore asm function
/// 
/// 第 11 行，在 trap_return 的开始处就调用 set_user_trap_entry ，来让应用 Trap 到 S 的时候可以跳转到 __alltraps 。
/// 第 18 行，展示了计算 __restore 虚地址的过程：
/// 由于 __alltraps 是对齐到地址空间跳板页面的起始地址 TRAMPOLINE 上的，
/// 则 __restore 的虚拟地址只需在 TRAMPOLINE 基础上加上 __restore 相对于 __alltraps 的偏移量即可。
/// 这里 __alltraps 和 __restore 都是指编译器在链接时看到的内核内存布局中的地址。
pub fn trap_return() -> ! {
    set_user_trap_entry();
    let trap_cx_ptr = TRAP_CONTEXT_BASE;
    let user_satp = current_user_token();
    extern "C" {
        fn __alltraps();
        fn __restore();
    }
    let restore_va = __restore as usize - __alltraps as usize + TRAMPOLINE;
    // trace!("[kernel] trap_return: ..before return");
    unsafe {
        asm!(
            "fence.i",
            "jr {restore_va}",         // jump to new addr of __restore asm function
            restore_va = in(reg) restore_va,
            in("a0") trap_cx_ptr,      // a0 = virt addr of Trap Context
            in("a1") user_satp,        // a1 = phy addr of usr page table
            options(noreturn)
        );
    }
}

#[no_mangle]
/// handle trap from kernel
/// Unimplement: traps/interrupts/exceptions from kernel mode
/// Todo: Chapter 9: I/O device
pub fn trap_from_kernel() -> ! {
    use riscv::register::sepc;
    trace!("stval = {:#x}, sepc = {:#x}", stval::read(), sepc::read());
    panic!("a trap {:?} from kernel!", scause::read().cause());
}

pub use context::TrapContext;
