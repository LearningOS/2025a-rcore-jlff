//! Implementation of [`MapArea`] and [`MemorySet`].

use super::{frame_alloc, FrameTracker};
use super::{PTEFlags, PageTable, PageTableEntry};
use super::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
use super::{StepByOne, VPNRange};
use crate::config::{
    KERNEL_STACK_SIZE, MEMORY_END, PAGE_SIZE, TRAMPOLINE, TRAP_CONTEXT_BASE, USER_STACK_SIZE,
};
use crate::sync::UPSafeCell;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::asm;
use lazy_static::*;
use riscv::register::satp;

extern "C" {
    fn stext();
    fn etext();
    fn srodata();
    fn erodata();
    fn sdata();
    fn edata();
    fn sbss_with_stack();
    fn ebss();
    fn ekernel();
    fn strampoline();
}

lazy_static! {
    /// The kernel's initial memory mapping(kernel address space)
    /// 我们创建内核地址空间的全局实例：
    /// 从之前对于 lazy_static! 宏的介绍可知， 
    /// KERNEL_SPACE 在运行期间它第一次被用到时才会实际进行初始化，
    /// 而它所 占据的空间则是编译期被放在全局数据段中。
    ///  Arc<UPSafeCell<_>> 同时带来 Arc<T> 提供的共享 引用，和 UPSafeCell<T> 提供的互斥访问。
    pub static ref KERNEL_SPACE: Arc<UPSafeCell<MemorySet>> =
        Arc::new(unsafe { UPSafeCell::new(MemorySet::new_kernel()) });
}
/// address space
/// 地址空间：一系列有关联的逻辑段
/// 地址空间 是一系列有关联的不一定连续的逻辑段，
/// 这种关联一般是指这些逻辑段组成的虚拟内存空间与一个运行的程序
/// （目前把一个运行的程序称为任务，后续会称为进程）绑定，
/// 即这个运行的程序对代码和数据的直接访问范围限制在它关联的虚拟地址空间之内。
/// 这样我们就有任务的地址空间，内核的地址空间等说法了。地址空间使用 MemorySet 类型来表示：
/// 它包含了该地址空间的多级页表 page_table 和一个逻辑段 MapArea 的向量 areas 。
/// 注意 PageTable 下挂着所有多级页表的节点所在的物理页帧，
/// 而每个 MapArea 下则挂着对应逻辑段中的数据所在的物理页帧，
/// 这两部分合在一起构成了一个地址空间所需的所有物理页帧。
/// 这同样是一种 RAII 风格，当一个地址空间 MemorySet 生命周期结束后，这些物理页帧都会被回收。
pub struct MemorySet {
    page_table: PageTable,
    areas: Vec<MapArea>,
}

impl MemorySet {
    /// Create a new empty `MemorySet`.
    /// new_bare 方法可以新建一个空的地址空间；
    pub fn new_bare() -> Self {
        Self {
            page_table: PageTable::new(),
            areas: Vec::new(),
        }
    }
    /// Get the page table token
    pub fn token(&self) -> usize {
        self.page_table.token()
    }
    /// Assume that no conflicts.
    /// insert_framed_area 方法调用 push ，
    /// 可以在当前地址空间插入一个 Framed 方式映射到 物理内存的逻辑段。
    /// 注意该方法的调用者要保证同一地址空间内的任意两个逻辑段不能存在交集，
    /// 从后面即将分别介绍的内核和 应用的地址空间布局可以看出这一要求得到了保证；
    pub fn insert_framed_area(
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission,
    ) {
        self.push(
            MapArea::new(start_va, end_va, MapType::Framed, permission),
            None,
        );
    }

    /// 
    pub fn remove_framed_area(&mut self, start_va: VirtAddr, end_va: VirtAddr) {
        self.remove(MapArea::new(start_va, end_va, MapType::Framed, MapPermission { bits: 0 }));
    }

    /// push 方法可以在当前地址空间插入一个新的逻辑段 map_area ，
    /// 如果它是以 Framed 方式映射到 物理内存，
    /// 还可以可选地在那些被映射到的物理页帧上写入一些初始化数据 data ；
    fn push(&mut self, mut map_area: MapArea, data: Option<&[u8]>) {
        map_area.map(&mut self.page_table);
        if let Some(data) = data {
            map_area.copy_data(&mut self.page_table, data);
        }
        self.areas.push(map_area);
    }
    
    fn remove(&mut self, mut map_area:MapArea) {
        map_area.unmap(&mut self.page_table);
        //self.areas.p
    }

    /// Mention that trampoline is not collected by areas.
    /// 这里我们为了实现方便并没有新增逻辑段 MemoryArea
    ///  而是直接在多级页表中插入一个从地址空间的最高虚拟页面映射到跳板汇编代码所在的物理页帧的键值对，
    /// 访问权限与代码段相同，即 RX （可读可执行）。
    fn map_trampoline(&mut self) {
        self.page_table.map(
            VirtAddr::from(TRAMPOLINE).into(),
            PhysAddr::from(strampoline as usize).into(),
            PTEFlags::R | PTEFlags::X,
        );
    }
    /// Without kernel stacks.
    /// new_kernel 可以生成内核的地址空间，
    /// new_kernel 将映射跳板和地址空间中最低 中的所有的逻辑段。
    /// 第 3 行开始，我们从 os/src/linker.ld 中引用了很多表示了各个段位置的符号，
    /// 而后在 new_kernel 中，我们从低地址到高地址 依次创建 5 个逻辑段并通过 push 方法将它们插入到内核地址空间中，
    /// 上面我们已经详细介绍过这 5 个逻辑段。
    /// 跳板 是通过 map_trampoline 方法来映射的，我们也将在本章最后一节进行讲解。
    /// 可以看到，跳板放在最高的一个虚拟页面中。
    /// 
    /// 四个逻辑段 .text/.rodata/.data/.bss 被恒等映射到物理内存，
    /// 这使得我们在无需调整内核内存布局 os/src/linker.ld 的情况下就仍能和启用页表机制之前那样访问内核的各数据段。
    /// 注意我们借用页表机制对这些逻辑段的访问方式做出了限制，这都是为了 在硬件的帮助下能够尽可能发现内核中的 bug ，
    /// 在这里：
    /// 
    /// 四个逻辑段的 U 标志位均未被设置，使得 CPU 只能在处于 S 特权级（或以上）时访问它们；
    /// 代码段 .text 不允许被修改；
    /// 只读数据段 .rodata 不允许被修改，也不允许从它上面取指；
    /// .data/.bss 均允许被读写，但是不允许从它上面取指。
    /// 此外， 之前 提到过内核地址空间中需要存在一个恒等映射到内核数据段之外的可用物理 页帧的逻辑段，
    /// 这样才能在启用页表机制之后，内核仍能以纯软件的方式读写这些物理页帧。
    /// 它们的标志位仅包含 rw ，意味着该 逻辑段只能在 S 特权级以上访问，并且只能读写。
    pub fn new_kernel() -> Self {
        let mut memory_set = Self::new_bare();
        // map trampoline
        info!(".trampoline VA:{:#x}, PA:{:#x})", TRAMPOLINE, strampoline as usize);
        memory_set.map_trampoline();
        // map kernel sections
        info!(".text [{:#x}, {:#x})", stext as usize, etext as usize);
        info!(".rodata [{:#x}, {:#x})", srodata as usize, erodata as usize);
        info!(".data [{:#x}, {:#x})", sdata as usize, edata as usize);
        info!(
            ".bss [{:#x}, {:#x})",
            sbss_with_stack as usize, ebss as usize
        );
        info!("mapping .text section");
        memory_set.push(
            MapArea::new(
                (stext as usize).into(),
                (etext as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::X,
            ),
            None,
        );
        info!("mapping .rodata section");
        memory_set.push(
            MapArea::new(
                (srodata as usize).into(),
                (erodata as usize).into(),
                MapType::Identical,
                MapPermission::R,
            ),
            None,
        );
        info!("mapping .data section");
        memory_set.push(
            MapArea::new(
                (sdata as usize).into(),
                (edata as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        info!("mapping .bss section");
        memory_set.push(
            MapArea::new(
                (sbss_with_stack as usize).into(),
                (ebss as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        info!("mapping physical memory");
        memory_set.push(
            MapArea::new(
                (ekernel as usize).into(),
                MEMORY_END.into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        memory_set
    }

    /// Include sections in elf and trampoline and TrapContext and user stack,
    /// also returns user_sp_base and entry point.
    /// from_elf 则可以应用的 ELF 格式可执行文件 解析出各数据段并对应生成应用的地址空间。
    /// 它们的实现我们将在后面讨论。
    /// 
    /// 左侧给出了应用地址空间最低 的布局：从 开始向高地址放置应用内存布局中的 各个逻辑段，
    /// 最后放置带有一个保护页面的用户栈。
    /// 这些逻辑段都是以 Framed 方式映射到物理内存的，从访问方式上来说都加上 了 U 标志位代表 CPU 可以在 U 特权级也就是执行应用代码的时候访问它们。
    /// 
    /// 右侧则给出了最高的 ， 可以看出它只是和内核地址空间一样将跳板放置在最高页，
    /// 还将 Trap 上下文放置在次高页中。
    /// 这两个虚拟页面虽然位于应用地址空间， 但是它们并不包含 U 标志位，
    /// 事实上它们在地址空间切换的时候才会发挥作用，请同样参考本章的最后一节。
    /// 
    /// 第 9 行，我们将跳板插入到应用地址空间；
    /// 第 11 行，我们使用外部 crate xmas_elf 来解析传入的应用 ELF 数据并可以轻松取出各个部分。 
    /// 此前 我们简要介绍过 ELF 格式的布局。
    /// 第 14 行，我们取出 ELF 的魔数来判断 它是不是一个合法的 ELF 。
    /// 第 15 行，我们可以直接得到 program header 的数目，
    /// 然后遍历所有的 program header 并将合适的区域加入 到应用地址空间中。这一过程的主体在第 17~39 行之间。
    /// 第 19 行我们确认 program header 的类型是 LOAD ， 这表明它有被内核加载的必要，此时不必理会其他类型的 program header 。
    /// 接着通过 ph.virtual_addr() 和 ph.mem_size() 来计算这一区域在应用地址空间中的位置，
    /// 通过 ph.flags() 来确认这一区域访问方式的 限制并将其转换为 MapPermission 类型（注意它默认包含 U 标志位）。
    /// 最后我们在第 27 行创建逻辑段 map_area 并在第 34 行 push 到应用地址空间。
    ///  push 的时候我们需要完成数据拷贝，当前 program header 数据被存放的位置可以通过 ph.offset() 和 ph.file_size() 来找到。 
    /// 注意当 存在一部分零初始化的时候， ph.file_size() 将会小于 ph.mem_size() ，
    /// 因为这些零出于缩减可执行 文件大小的原因不应该实际出现在 ELF 数据中。
    /// 
    /// 我们从第 40 行开始处理用户栈。
    /// 注意在前面加载各个 program header 的时候，我们就已经维护了 max_end_vpn 记录目前涉及到的最大的虚拟页号，
    /// 只需紧接着在它上面再放置一个保护页面和用户栈即可。
    /// 第 53 行则在应用地址空间中映射次高页面来存放 Trap 上下文。
    /// 第 59 行返回的时候，我们不仅返回应用地址空间 memory_set ，
    /// 也同时返回用户栈虚拟地址 user_stack_top 以及从解析 ELF 得到的该应用入口点地址，
    /// 它们将被我们用来创建应用的任务控制块。
    pub fn from_elf(elf_data: &[u8]) -> (Self, usize, usize) {
        let mut memory_set = Self::new_bare();
        // map trampoline
        memory_set.map_trampoline();
        // map program headers of elf, with U flag
        let elf = xmas_elf::ElfFile::new(elf_data).unwrap();
        let elf_header = elf.header;
        let magic = elf_header.pt1.magic;
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");
        let ph_count = elf_header.pt2.ph_count();
        let mut max_end_vpn = VirtPageNum(0);
        for i in 0..ph_count {
            let ph = elf.program_header(i).unwrap();
            if ph.get_type().unwrap() == xmas_elf::program::Type::Load {
                let start_va: VirtAddr = (ph.virtual_addr() as usize).into();
                let end_va: VirtAddr = ((ph.virtual_addr() + ph.mem_size()) as usize).into();
                let mut map_perm = MapPermission::U;
                let ph_flags = ph.flags();
                if ph_flags.is_read() {
                    map_perm |= MapPermission::R;
                }
                if ph_flags.is_write() {
                    map_perm |= MapPermission::W;
                }
                if ph_flags.is_execute() {
                    map_perm |= MapPermission::X;
                }
                let map_area = MapArea::new(start_va, end_va, MapType::Framed, map_perm);
                max_end_vpn = map_area.vpn_range.get_end();
                memory_set.push(
                    map_area,
                    Some(&elf.input[ph.offset() as usize..(ph.offset() + ph.file_size()) as usize]),
                );
            }
        }
        // map user stack with U flags
        let max_end_va: VirtAddr = max_end_vpn.into();
        let mut user_stack_bottom: usize = max_end_va.into();
        // guard page
        user_stack_bottom += PAGE_SIZE;
        let user_stack_top = user_stack_bottom + USER_STACK_SIZE;
        memory_set.push(
            MapArea::new(
                user_stack_bottom.into(),
                user_stack_top.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W | MapPermission::U,
            ),
            None,
        );
        // used in sbrk
        memory_set.push(
            MapArea::new(
                user_stack_top.into(),
                user_stack_top.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W | MapPermission::U,
            ),
            None,
        );
        // map TrapContext
        memory_set.push(
            MapArea::new(
                TRAP_CONTEXT_BASE.into(),
                TRAMPOLINE.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        (
            memory_set,
            user_stack_top,
            elf.header.pt2.entry_point() as usize,
        )
    }
    /// Change page table by writing satp CSR Register.
    /// PageTable::token 会按照 satp CSR 格式要求 构造一个无符号 64 位无符号整数，
    /// 使得其 分页模式为 SV39 ，且将当前多级页表的根节点所在的物理页号填充进去。
    /// 在 activate 中，我们将这个值写入当前 CPU 的 satp CSR ，
    /// 从这一刻开始 SV39 分页模式就被启用了，而且 MMU 会使用内核地址空间的多级页表进行地址转换。

    /// 我们必须注意切换 satp CSR 是否是一个 平滑 的过渡：
    /// 其含义是指，切换 satp 的指令及其下一条指令这两条相邻的指令的 虚拟地址是相邻的
    /// （由于切换 satp 的指令并不是一条跳转指令， pc 只是简单的自增当前指令的字长）， 
    /// 而它们所在的物理地址一般情况下也是相邻的，
    /// 但是它们所经过的地址转换流程却是不同的——切换 satp 导致 MMU 查的多级页表 是不同的。
    /// 这就要求前后两个地址空间在切换 satp 的指令 附近 的映射满足某种意义上的连续性。
    /// 幸运的是，我们做到了这一点。
    /// 这条写入 satp 的指令及其下一条指令都在内核内存布局的代码段中，在切换之后是一个恒等映射，
    ///  而在切换之前是视为物理地址直接取指，也可以将其看成一个恒等映射。
    /// 这完全符合我们的期待：即使切换了地址空间，指令仍应该 能够被连续的执行。

    /// 注意到在 activate 的最后，我们插入了一条汇编指令 sfence.vma ，它又起到什么作用呢？
    /// 让我们再来回顾一下多级页表：它相比线性表虽然大量节约了内存占用，但是却需要 MMU 进行更多的隐式访存。
    /// 如果是一个线性表， MMU 仅需单次访存就能找到页表项并完成地址转换，
    /// 而多级页表（以 SV39 为例，不考虑大页）最顺利的情况下也需要三次访存。
    /// 这些 额外的访存和真正访问数据的那些访存在空间上并不相邻，加大了多级缓存的压力，
    /// 一旦缓存缺失将带来巨大的性能惩罚。
    /// 如果采用 多级页表实现，这个问题会变得更为严重，使得地址空间抽象的性能开销过大。
    /// 为了解决性能问题，一种常见的做法是在 CPU 中利用部分硬件资源额外加入一个 快表 (TLB, Translation Lookaside Buffer) ，
    ///  它维护了部分虚拟页号到页表项的键值对。
    /// 当 MMU 进行地址转换的时候，首先 会到快表中看看是否匹配，如果匹配的话直接取出页表项完成地址转换而无需访存；
    /// 否则再去查页表并将键值对保存在快表中。
    /// 一旦 我们修改了 satp 切换了地址空间，快表中的键值对就会失效，因为它还表示着上个地址空间的映射关系。
    /// 为了 MMU 的地址转换 能够及时与 satp 的修改同步，我们可以选择立即使用 sfence.vma 指令将快表清空，
    /// 这样 MMU 就不会看到快表中已经 过期的键值对了。
    pub fn activate(&self) {
        let satp = self.page_table.token();
        unsafe {
            satp::write(satp);
            asm!("sfence.vma");
        }
    }
    /// Translate a virtual page number to a page table entry
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.page_table.translate(vpn)
    }
    /// shrink the area to new_end
    #[allow(unused)]
    pub fn shrink_to(&mut self, start: VirtAddr, new_end: VirtAddr) -> bool {
        if let Some(area) = self
            .areas
            .iter_mut()
            .find(|area| area.vpn_range.get_start() == start.floor())
        {
            area.shrink_to(&mut self.page_table, new_end.ceil());
            true
        } else {
            false
        }
    }

    /// append the area to new_end
    #[allow(unused)]
    pub fn append_to(&mut self, start: VirtAddr, new_end: VirtAddr) -> bool {
        if let Some(area) = self
            .areas
            .iter_mut()
            .find(|area| area.vpn_range.get_start() == start.floor())
        {
            area.append_to(&mut self.page_table, new_end.ceil());
            true
        } else {
            false
        }
    }
    
    fn addr_in_range(&self, addr:usize, len:usize, range: VPNRange) -> bool {
        info!("addr_in_range?: {:#x}, {:#x} [{:#x}, {:#x})", 
                addr, 
                len,
                usize::from(range.get_start()), 
                usize::from(range.get_end()));
        addr < VirtAddr::from(range.get_end()).into() &&
        addr + len >= VirtAddr::from(range.get_start()).into()
    }
    /// 
    pub fn is_mapped(&self, start: usize, len:usize) -> bool {
        let count = self.areas.
        iter().
        find(|area| 
            self.addr_in_range(start, len, area.vpn_range)
        )
        .into_iter()
        .count();
        info!("count {}", count);
        count > 0
    }
    
    ///
    fn addr_in_range_unmap(&self, addr:usize, len:usize, range: VPNRange) -> bool {
        info!("addr_in_range?: {:#x}, {:#x} [{:#x}, {:#x})", 
                addr, 
                len,
                usize::from(range.get_start()), 
                usize::from(range.get_end()));
        addr >= VirtAddr::from(range.get_start()).into() &&
        addr + len <= VirtAddr::from(range.get_end()).into()
    }
    ///
    pub fn can_munmap(&self, start: usize, len:usize) -> bool {
        let count = self.areas.
        iter().
        find(|area| 
            self.addr_in_range_unmap(start, len, area.vpn_range)
        )
        .into_iter()
        .count();
        info!("count {}", count);
        count > 0
    }

}
/// map area structure, controls a contiguous piece of virtual memory
/// 逻辑段：一段连续地址的虚拟内存
/// 我们以逻辑段 MapArea 为单位描述一段连续地址的虚拟内存。
/// 所谓逻辑段，就是指地址区间中的一段实际可用（即 MMU 通过查多级页表可以正确完成地址转换）
/// 的地址连续的虚拟地址区间，该区间内包含的所有虚拟页面都以一种相同的方式映射到物理页帧，
/// 具有可读/可写/可执行等属性。
/// 
/// 其中 VPNRange 描述一段虚拟页号的连续区间，表示该逻辑段在地址区间中的位置和长度。
/// 它是一个迭代器，可以使用 Rust 的语法糖 for-loop 进行迭代。
/// 有兴趣的同学可以参考 os/src/mm/address.rs 中它的实现。
pub struct MapArea {
    vpn_range: VPNRange,
    data_frames: BTreeMap<VirtPageNum, FrameTracker>,
    map_type: MapType,
    map_perm: MapPermission,
}

impl MapArea {
    pub fn new(
        start_va: VirtAddr,
        end_va: VirtAddr,
        map_type: MapType,
        map_perm: MapPermission,
    ) -> Self {
        let start_vpn: VirtPageNum = start_va.floor();
        let end_vpn: VirtPageNum = end_va.ceil();
        Self {
            vpn_range: VPNRange::new(start_vpn, end_vpn),
            data_frames: BTreeMap::new(),
            map_type,
            map_perm,
        }
    }
    /// 对于第 4 行的 map_one 来说，在虚拟页号 vpn 已经确定的情况下，
    /// 它需要知道要将一个怎么样的页表项插入多级页表。 
    /// 页表项的标志位来源于当前逻辑段的类型为 MapPermission 的统一配置，只需将其转换为 PTEFlags ；
    /// 而页表项的 物理页号则取决于当前逻辑段映射到物理内存的方式：
    /// 
    /// 当以恒等映射 Identical 方式映射的时候，物理页号就等于虚拟页号；
    /// 当以 Framed 方式映射的时候，需要分配一个物理页帧让当前的虚拟页面可以映射过去，
    /// 此时页表项中的物理页号自然就是 这个被分配的物理页帧的物理页号。
    /// 此时还需要将这个物理页帧挂在逻辑段的 data_frames 字段下。
    /// 当确定了页表项的标志位和物理页号之后，即可调用多级页表 PageTable 的 map 接口来插入键值对。
    pub fn map_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        let ppn: PhysPageNum;
        match self.map_type {
            MapType::Identical => {
                ppn = PhysPageNum(vpn.0);
            }
            MapType::Framed => {
                let frame = frame_alloc().unwrap();
                ppn = frame.ppn;
                self.data_frames.insert(vpn, frame);
            }
        }
        let pte_flags = PTEFlags::from_bits(self.map_perm.bits).unwrap();
        page_table.map(vpn, ppn, pte_flags);
    }
    #[allow(unused)]
    pub fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        if self.map_type == MapType::Framed {
            self.data_frames.remove(&vpn);
        }
        page_table.unmap(vpn);
    }
    pub fn map(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.map_one(page_table, vpn);
        }
    }
    #[allow(unused)]
    pub fn unmap(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.unmap_one(page_table, vpn);
        }
    }
    #[allow(unused)]
    pub fn shrink_to(&mut self, page_table: &mut PageTable, new_end: VirtPageNum) {
        for vpn in VPNRange::new(new_end, self.vpn_range.get_end()) {
            self.unmap_one(page_table, vpn)
        }
        self.vpn_range = VPNRange::new(self.vpn_range.get_start(), new_end);
    }
    #[allow(unused)]
    pub fn append_to(&mut self, page_table: &mut PageTable, new_end: VirtPageNum) {
        for vpn in VPNRange::new(self.vpn_range.get_end(), new_end) {
            self.map_one(page_table, vpn)
        }
        self.vpn_range = VPNRange::new(self.vpn_range.get_start(), new_end);
    }
    /// data: start-aligned but maybe with shorter length
    /// assume that all frames were cleared before
    pub fn copy_data(&mut self, page_table: &mut PageTable, data: &[u8]) {
        assert_eq!(self.map_type, MapType::Framed);
        let mut start: usize = 0;
        let mut current_vpn = self.vpn_range.get_start();
        let len = data.len();
        loop {
            let src = &data[start..len.min(start + PAGE_SIZE)];
            let dst = &mut page_table
                .translate(current_vpn)
                .unwrap()
                .ppn()
                .get_bytes_array()[..src.len()];
            dst.copy_from_slice(src);
            start += PAGE_SIZE;
            if start >= len {
                break;
            }
            current_vpn.step();
        }
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
/// map type for memory set: identical or framed
pub enum MapType {
    Identical,
    Framed,
}

bitflags! {
    /// map permission corresponding to that in pte: `R W X U`
    /// MapPermission 表示控制该逻辑段的访问方式，
    /// 它是页表项标志位 PTEFlags 的一个子集，仅保留 U/R/W/X 四个标志位，
    /// 因为其他的标志位仅与硬件的地址转换机制细节相关，这样的设计能避免引入错误的标志位。
    pub struct MapPermission: u8 {
        ///Readable
        const R = 1 << 1;
        ///Writable
        const W = 1 << 2;
        ///Excutable
        const X = 1 << 3;
        ///Accessible in U mode
        const U = 1 << 4;
    }
}

/// Return (bottom, top) of a kernel stack in kernel space.
pub fn kernel_stack_position(app_id: usize) -> (usize, usize) {
    let top = TRAMPOLINE - app_id * (KERNEL_STACK_SIZE + PAGE_SIZE);
    let bottom = top - KERNEL_STACK_SIZE;
    (bottom, top)
}

/// remap test in kernel space
#[allow(unused)]
pub fn remap_test() {
    let mut kernel_space = KERNEL_SPACE.exclusive_access();
    let mid_text: VirtAddr = ((stext as usize + etext as usize) / 2).into();
    let mid_rodata: VirtAddr = ((srodata as usize + erodata as usize) / 2).into();
    let mid_data: VirtAddr = ((sdata as usize + edata as usize) / 2).into();
    assert!(!kernel_space
        .page_table
        .translate(mid_text.floor())
        .unwrap()
        .writable(),);
    assert!(!kernel_space
        .page_table
        .translate(mid_rodata.floor())
        .unwrap()
        .writable(),);
    assert!(!kernel_space
        .page_table
        .translate(mid_data.floor())
        .unwrap()
        .executable(),);
    println!("remap_test passed!");
}
