//! RISC-V timer-related functionality

use crate::config::CLOCK_FREQ;
use crate::sbi::set_timer;
use riscv::register::time;
/// The number of ticks per second
const TICKS_PER_SEC: usize = 100;
/// The number of milliseconds per second
const MSEC_PER_SEC: usize = 1000;
/// The number of microseconds per second
#[allow(dead_code)]
const MICRO_PER_SEC: usize = 1_000_000;

/// Get the current time in ticks
/// RISC-V 要求处理器维护时钟计数器 mtime，还有另外一个 CSR mtimecmp 。 
/// 一旦计数器 mtime 的值超过了 mtimecmp，就会触发一次时钟中断。
/// 运行在 M 特权级的 SEE 已经预留了相应的接口，基于此编写的 get_time 函数可以取得当前 mtime 计数器的值；
pub fn get_time() -> usize {
    time::read()
}

/// get current time in milliseconds
#[allow(dead_code)]
pub fn get_time_ms() -> usize {
    time::read() * MSEC_PER_SEC / CLOCK_FREQ
}

/// get current time in microseconds
/// get_time_us 可以以微秒为单位返回当前计数器的值。
#[allow(dead_code)]
pub fn get_time_us() -> usize {
    time::read() * MICRO_PER_SEC / CLOCK_FREQ
}

/// Set the next timer interrupt
/// 第 5 行， sbi 子模块有一个 set_timer 调用，用来设置 mtimecmp 的值。
/// 第 14 行， timer 子模块的 set_next_trigger 函数对 set_timer 进行了封装，
/// 它首先读取当前 mtime 的值，然后计算出 10ms 之内计数器的增量，再将 mtimecmp 设置为二者的和。
/// 这样，10ms 之后一个 S 特权级时钟中断就会被触发。
/// 至于增量的计算方式， CLOCK_FREQ 是一个预先获取到的各平台不同的时钟频率，单位为赫兹，也就是一秒钟之内计数器的增量。
/// 它可以在 config 子模块中找到。10ms 的话只需除以常数 TICKS_PER_SEC 也就是 100 即可。
pub fn set_next_trigger() {
    set_timer(get_time() + CLOCK_FREQ / TICKS_PER_SEC);
}
