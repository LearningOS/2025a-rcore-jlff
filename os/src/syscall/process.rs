//! Process management syscalls
use crate::{
    task::{exit_current_and_run_next, get_syscall_num, suspend_current_and_run_next},
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

// 调用规范：
// 这个系统调用有三种功能，根据 trace_request 的值不同，执行不同的操作：

// 如果 trace_request 为 0，则 id 应被视作 *const u8 ，表示读取当前任务 id 地址处一个字节的无符号整数值。此时应忽略 data 参数。返回值为 id 地址处的值。

// 如果 trace_request 为 1，则 id 应被视作 *const u8 ，表示写入 data （作为 u8，即只考虑最低位的一个字节）到该用户程序 id 地址处。返回值应为0。

// 如果 trace_request 为 2，表示查询当前任务调用编号为 id 的系统调用的次数，返回值为这个调用次数。本次调用也计入统计 。

// 否则，忽略其他参数，返回值为 -1。
// TODO: implement the syscall

// 说明：
// 你可能会注意到，这个调用的读写并不安全，使用不当可能导致崩溃。这是因为在下一章节实现地址空间之前，系统中缺乏隔离机制。所以我们 不要求你实现安全检查机制，只需通过测试用例即可 。

// 你还可能注意到，这个系统调用读写本任务内存的功能并不是很有用。这是因为作业的灵感来源 syscall 主要依靠 trace 功能追踪其他任务的信息，但在本章节我们还没有进程、线程等概念，所以简化了操作，只要求追踪自身的信息。
// 提示

// 大胆修改已有框架！除了配置文件，你几乎可以随意修改已有框架的内容。

// 系统调用次数可以考虑在内核态的 syscall 函数中统计。

// 可以扩展 TaskManagerInner 中的结构来维护新的信息。

// 不要害怕使用 unsafe 做类型转换，这在内核处理用户调用时是不可避免的。

// 在实现时，可以把系统调用参数中前缀的下划线去掉，这样更清晰。实验框架之所以这么写，是因为在没有使用对应参数的情况下，Rust 推荐使用下划线前缀以避免警告。
pub fn sys_trace(trace_request: usize, id: usize, _data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        0 => {assert!(false); return 0;}
        1 => {assert!(false); return 1;}
        2 => {return get_syscall_num(id);  }
        _ => {return -1;}
    }
}
