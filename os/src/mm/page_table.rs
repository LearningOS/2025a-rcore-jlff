//! Implementation of [`PageTableEntry`] and [`PageTable`].

use super::{frame_alloc, FrameTracker, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
use alloc::vec;
use alloc::vec::Vec;
use bitflags::*;

bitflags! {
    /// page table entry flags
    /// 上图为 SV39 分页模式下的页表项，其中这 位是物理页号，最低的 位 则是标志位，
    /// 它们的含义如下
    /// （请注意，为方便说明，下文我们用 页表项的对应虚拟页面 来表示索引到一个页表项的虚拟页号对应的虚拟页面）：

    /// V(Valid)：仅当位 V 为 1 时，页表项才是合法的；
    /// R(Read)/W(Write)/X(eXecute)：分别控制索引到这个页表项的对应虚拟页面是否允许读/写/执行；
    /// U(User)：控制索引到这个页表项的对应虚拟页面是否在 CPU 处于 U 特权级的情况下是否被允许访问；
    /// G：暂且不理会；
    /// A(Accessed)：处理器记录自从页表项上的这一位被清零之后，页表项的对应虚拟页面是否被访问过；
    /// D(Dirty)：处理器记录自从页表项上的这一位被清零之后，页表项的对应虚拟页面是否被修改过。
    /// 除了 G 外的上述位可以被操作系统设置，只有 A 位和 D 位会被处理器动态地直接设置为 1 ，
    /// 表示对应的页被访问过或修过（ 注：A 位和 D 位能否被处理器硬件直接修改，取决于处理器的具体实现）。
    /// 让我们先来实现页表项中的标志位 PTEFlags ：
    /// 
    /// bitflags 是一个 Rust 中常用来比特标志位的 crate 。它提供了一个 bitflags! 宏，
    /// 如上面的代码段所展示的那样，可以将一个 u8 封装成一个标志位的集合类型，支持一些常见的集合运算。
    pub struct PTEFlags: u8 {
        /// Valid
        const V = 1 << 0;
        /// Readable
        const R = 1 << 1;
        /// Writable
        const W = 1 << 2;
        /// eXecutable
        const X = 1 << 3;
        /// User
        const U = 1 << 4;
        /// Global
        const G = 1 << 5;
        /// Accessed
        const A = 1 << 6;
        /// Dirty
        const D = 1 << 7;
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
/// page table entry structure
/// 第 3 行我们让编译器自动为 PageTableEntry 实现 Copy/Clone Trait，
/// 来让这个类型以值语义赋值/传参的时候 不会发生所有权转移，而是拷贝一份新的副本
pub struct PageTableEntry {
    /// bits of page table entry
    pub bits: usize,
}

impl PageTableEntry {
    /// Create a new page table entry
    /// 第 10 行使得我们可以从一个物理页号 PhysPageNum 和一个页表项标志位 PTEFlags 生成一个页表项 PageTableEntry 实例；而第 20 行和第 23 行则分别可以从一个页表项将它们两个取出。
    pub fn new(ppn: PhysPageNum, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: ppn.0 << 10 | flags.bits as usize,
        }
    }
    /// Create an empty page table entry
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }
    /// Get the physical page number from the page table entry
    pub fn ppn(&self) -> PhysPageNum {
        (self.bits >> 10 & ((1usize << 44) - 1)).into()
    }
    /// Get the flags from the page table entry
    pub fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits(self.bits as u8).unwrap()
    }
    /// The page pointered by page table entry is valid?
    /// 后面我们还为 PageTableEntry 实现了一些辅助函数(Helper Function)，
    /// 可以快速判断一个页表项的 V/R/W/X 标志位是否为 1，以 V 标志位的判断为例：
    pub fn is_valid(&self) -> bool {
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is readable?
    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is writable?
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is executable?
    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }

    /// is_user
    pub fn is_user(&self) -> bool {
        (self.flags() & PTEFlags::U) != PTEFlags::empty()
    }

}

/// page table structure
pub struct PageTable {
    root_ppn: PhysPageNum,
    frames: Vec<FrameTracker>,
}

/// Assume that it won't oom when creating/mapping.
impl PageTable {
    /// Create a new page table
    /// 当我们通过 new 方法新建一个 PageTable 的时候，它只需有一个根节点。
    /// 为此我们需要分配一个物理页帧 FrameTracker 并挂在向量 frames 下，然后更新根节点的物理页号 root_ppn 。
    pub fn new() -> Self {
        let frame = frame_alloc().unwrap();
        PageTable {
            root_ppn: frame.ppn,
            frames: vec![frame],
        }
    }
    /// Temporarily used to get arguments from user space.
    pub fn from_token(satp: usize) -> Self {
        Self {
            root_ppn: PhysPageNum::from(satp & ((1usize << 44) - 1)),
            frames: Vec::new(),
        }
    }
    /// Find PageTableEntry by VirtPageNum, create a frame for a 4KB page table if not exist
    /// PageTable::find_pte_create 在多级页表找到一个虚拟页号对应的页表项的可变引用方便后续的读写。
    /// 如果在 遍历的过程中发现有节点尚未创建则会新建一个节点。
    /// 变量 ppn 表示当前节点的物理页号，最开始指向多级页表的根节点。
    /// 随后每次循环通过 get_pte_array 将 取出当前节点的页表项数组，并根据当前级页索引找到对应的页表项。
    /// 如果当前节点是一个叶节点，那么直接返回这个页表项 的可变引用；否则尝试向下走。
    /// 走不下去的话就新建一个节点，更新作为下级节点指针的页表项，并将新分配的物理页帧移动到 向量 frames 中方便后续的自动回收。
    /// 注意在更新页表项的时候，不仅要更新物理页号，还要将标志位 V 置 1， 
    /// 不然硬件在查多级页表的时候，会认为这个页表项不合法，从而触发 Page Fault 而不能向下走。
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                let frame = frame_alloc().unwrap();
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V);
                self.frames.push(frame);
            }
            ppn = pte.ppn();
        }
        result
    }
    /// Find PageTableEntry by VirtPageNum
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                return None;
            }
            ppn = pte.ppn();
        }
        result
    }
    /// set the map between virtual page number and physical page number
    /// 多级页表并不是被创建出来之后就不再变化的，为了 MMU 能够通过地址转换正确找到应用地址空间中的数据实际被内核放在内存中 位置，
    /// 操作系统需要动态维护一个虚拟页号到页表项的映射，支持插入/删除键值对，其方法签名如下：
    /// 我们通过 map 方法来在多级页表中插入一个键值对，注意这里我们将物理页号 ppn 和页表项标志位 flags 作为 不同的参数传入而不是整合为一个页表项；
    /// 相对的，我们通过 unmap 方法来删除一个键值对，在调用时仅需给出作为索引的虚拟页号即可。
    #[allow(unused)]
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags) {
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V);
    }
    /// remove the map between virtual page number and physical page number
    #[allow(unused)]
    pub fn unmap(&mut self, vpn: VirtPageNum) {
        let pte = self.find_pte(vpn).unwrap();
        assert!(pte.is_valid(), "vpn {:?} is invalid before unmapping", vpn);
        *pte = PageTableEntry::empty();
    }
    /// get the page table entry from the virtual page number
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.find_pte(vpn).map(|pte| *pte)
    }
    /// get the token from the page table
    /// PageTable::token 会按照 satp CSR 格式要求 构造一个无符号 64 位无符号整数，
    /// 使得其 分页模式为 SV39 ，且将当前多级页表的根节点所在的物理页号填充进去。
    pub fn token(&self) -> usize {
        8usize << 60 | self.root_ppn.0
    }
}

/// Translate&Copy a ptr[u8] array with LENGTH len to a mutable u8 Vec through page table
/// 类似Trap处理的改进，由于内核和应用地址空间的隔离， sys_write 不再能够直接访问位于应用空间中的数据，而需要手动查页表才能知道那些数据被放置在哪些物理页帧上并进行访问。
/// 为此，页表模块 page_table 提供了将应用地址空间中一个缓冲区转化为在内核空间中能够直接访问的形式的辅助函数：
/// 参数中的 token 是某个应用地址空间的 token ， ptr 和 len 则分别表示该地址空间中的一段缓冲区的起始地址和长度
/// (注：这个缓冲区的应用虚拟地址范围是连续的)。 
/// translated_byte_buffer 会以向量的形式返回一组可以在内核空间中直接访问的字节数组切片
/// （注：这个缓冲区的内核虚拟地址范围有可能是不连续的），具体实现在这里不再赘述。
pub fn translated_byte_buffer(token: usize, ptr: *const u8, len: usize) -> Vec<&'static mut [u8]> {
    let page_table = PageTable::from_token(token);
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::new();
    while start < end {
        let start_va = VirtAddr::from(start);
        let mut vpn = start_va.floor();
        let ppn = page_table.translate(vpn).unwrap().ppn();
        vpn.step();
        let mut end_va: VirtAddr = vpn.into();
        end_va = end_va.min(VirtAddr::from(end));
        if end_va.page_offset() == 0 {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..]);
        } else {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..end_va.page_offset()]);
        }
        start = end_va.into();
    }
    v
}
