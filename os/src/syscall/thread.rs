use crate::{
    mm::kernel_token,
    task::{add_task, current_task, TaskControlBlock},
    trap::{trap_handler, TrapContext},
};
use alloc::{sync::Arc, vec::Vec};
/// thread create syscall
/// 第二种创建线程的方式是通过 thread_create 系统调用。
/// 重点是需要了解创建线程控制块，在线程控制块中初始化各个成员变量，建立好进程和线程的关系等。
/// 只有建立好这些成员变量，才能给线程建立一个灵活方便的执行环境。
/// 这里列出支持线程正确运行所需的重要的执行环境要素：
// 线程的用户态栈：确保在用户态的线程能正常执行函数调用；
// 线程的内核态栈：确保线程陷入内核后能正常执行函数调用；
// 线程共享的跳板页和线程独占的 Trap 上下文：确保线程能正确的进行用户态与内核态间的切换；
// 线程的任务上下文：线程在内核态的寄存器信息，用于线程切换。
pub fn sys_thread_create(entry: usize, arg: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_thread_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let task = current_task().unwrap();
    let process = task.process.upgrade().unwrap();
    // create a new thread
    let new_task = Arc::new(TaskControlBlock::new(
        Arc::clone(&process),
        task.inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .ustack_base,
        true,
    ));
    // add new task to scheduler
    add_task(Arc::clone(&new_task));
    let new_task_inner = new_task.inner_exclusive_access();
    let new_task_res = new_task_inner.res.as_ref().unwrap();
    let new_task_tid = new_task_res.tid;
    let mut process_inner = process.inner_exclusive_access();
    // add new thread to current process
    let tasks = &mut process_inner.tasks;
    while tasks.len() < new_task_tid + 1 {
        tasks.push(None);
    }
    tasks[new_task_tid] = Some(Arc::clone(&new_task));

    if process_inner.is_enable_deadlock_detect {
        // availave
        // allocation
        // need
        process_inner.allocation.push(Vec::new());
        process_inner.need.push(Vec::new());
        
    }


    // 第25~32行，初始化位于该线程在用户态地址空间中的 Trap 上下文：
    // 设置线程的函数入口点和用户栈， 使得第一次进入用户态时能从线程起始位置开始正确执行；
    // 设置好内核栈和陷入函数指针 trap_handler ， 保证在 Trap 的时候用户态的线程能正确进入内核态。
    let new_task_trap_cx = new_task_inner.get_trap_cx();
    *new_task_trap_cx = TrapContext::app_init_context(
        entry,
        new_task_res.ustack_top(),
        kernel_token(),
        new_task.kstack.get_top(),
        trap_handler as usize,
    );
    (*new_task_trap_cx).x[10] = arg;
    new_task_tid as isize
}
/// get current thread id syscall
pub fn sys_gettid() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_gettid",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid as isize
}

/// wait for a thread to exit syscall
///
/// thread does not exist, return -1
/// thread has not exited yet, return -2
/// otherwise, return thread's exit code
pub fn sys_waittid(tid: usize) -> i32 {
    trace!(
        "kernel:pid[{}] tid[{}] sys_waittid",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let task = current_task().unwrap();
    let process = task.process.upgrade().unwrap();
    let task_inner = task.inner_exclusive_access();
    let mut process_inner = process.inner_exclusive_access();
    // a thread cannot wait for itself
    if task_inner.res.as_ref().unwrap().tid == tid {
        return -1;
    }
    let mut exit_code: Option<i32> = None;
    let waited_task = process_inner.tasks[tid].as_ref();
    if let Some(waited_task) = waited_task {
        if let Some(waited_exit_code) = waited_task.inner_exclusive_access().exit_code {
            exit_code = Some(waited_exit_code);
        }
    } else {
        // waited thread does not exist
        return -1;
    }
    if let Some(exit_code) = exit_code {
        // dealloc the exited thread
        process_inner.tasks[tid] = None;
        exit_code
    } else {
        // waited thread has not exited
        -2
    }
}
