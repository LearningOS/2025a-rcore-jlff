//! virtio_blk device driver

mod virtio_blk;

pub use virtio_blk::VirtIOBlock;

use alloc::sync::Arc;
use easy_fs::BlockDevice;
use lazy_static::*;

/// 针对内核所要运行在的 qemu 或 k210 平台，
/// 我们需要将平台上的块设备驱动起来并实现 easy-fs 所需的 BlockDevice Trait ，这样 easy-fs 才能将该块设备用作 easy-fs 镜像的载体。
/// 
/// qemu 和 k210 平台上的块设备是不同的。
/// 在 qemu 上，我们使用 VirtIOBlock 访问 VirtIO 块设备；
/// 而在 k210 上，我们使用 SDCardWrapper 来访问插入 k210 开发板上真实的 microSD 卡，
/// 它们都实现了 easy-fs 要求的 BlockDevice Trait 。
/// 通过 #[cfg(feature)] 可以在编译的时候根据编译参数调整 BlockDeviceImpl 具体为哪个块设备，
/// 之后将它全局实例化为 BLOCK_DEVICE ，使得内核的其他模块可以访问。
/// 
type BlockDeviceImpl = virtio_blk::VirtIOBlock;

lazy_static! {
    /// The global block device driver instance: BLOCK_DEVICE with BlockDevice trait
    pub static ref BLOCK_DEVICE: Arc<dyn BlockDevice> = Arc::new(BlockDeviceImpl::new());
}

#[allow(unused)]
/// Test the block device
pub fn block_device_test() {
    let block_device = BLOCK_DEVICE.clone();
    let mut write_buffer = [0u8; 512];
    let mut read_buffer = [0u8; 512];
    for i in 0..512 {
        for byte in write_buffer.iter_mut() {
            *byte = i as u8;
        }
        block_device.write_block(i as usize, &write_buffer);
        block_device.read_block(i as usize, &mut read_buffer);
        assert_eq!(write_buffer, read_buffer);
    }
    println!("block device test passed!");
}
