use crate::{debugconf, xhci};
use core::{ptr::read_volatile, ptr::NonNull, time::Duration};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use crab_usb::{impl_trait, BoxFuture, Kernel, USBHost};
use dma_api::{Direction, Osal};
use futures::FutureExt;
use spin::{Mutex, Once};

const LEGACY_DMA_MASK: usize = 0xFFFF_FFFF;

static USB_INIT: Once<()> = Once::new();
static USB_HOST: Mutex<Option<USBHost>> = Mutex::new(None);
static DMA_OSAL_ONCE: Once<()> = Once::new();

fn ensure_dma_api_initialized() {
    DMA_OSAL_ONCE.call_once(|| {
        dma_api::init(&DMA_OSAL);
    });
}

struct DmaOsal;

impl Osal for DmaOsal {
    fn map(&self, addr: NonNull<u8>, _size: usize, _direction: Direction) -> u64 {
        match crate::dma::virt_to_phys(addr.as_ptr()) {
            Some(phys) => phys,
            None => {
                debugconf!(
                    "usb: dma_osal map failed for virt=0x{:X}\n",
                    addr.as_ptr() as usize
                );
                0
            }
        }
    }

    unsafe fn alloc(&self, _dma_mask: u64, layout: core::alloc::Layout) -> *mut u8 {
        // dma-api uses this to allocate DMA buffers for rings/context structures.
        // We must return physically-contiguous memory and a CPU-usable pointer.
        let align = layout.align().max(64);
        match crate::dma::alloc(layout.size(), align) {
            Some((_phys, virt)) => virt,
            None => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        crate::dma::dealloc(ptr, layout.size());
    }

    fn unmap(&self, _addr: NonNull<u8>, _size: usize) {}
    fn flush(&self, _addr: NonNull<u8>, _size: usize) {}
    fn invalidate(&self, _addr: NonNull<u8>, _size: usize) {}
}

static DMA_OSAL: DmaOsal = DmaOsal;

pub fn init_crab_controller(spawner: &Spawner) {
    USB_INIT.call_once(|| {
        if let Some(info) = xhci::controller_info() {
            if let Err(e) = spawner.spawn(crab_usb_init_task(info)) {
                debugconf!("usb: failed to spawn init task: {:?}\n", e);
            }
        }
    });
}

fn dma_mask(supports_64bit: bool) -> usize {
    if supports_64bit {
        usize::MAX
    } else {
        LEGACY_DMA_MASK
    }
}

struct KernelImpl;

impl_trait! {
    impl Kernel for KernelImpl {
        fn sleep<'a>(duration: Duration) -> BoxFuture<'a, ()> {
            // Use embassy-time to back off instead of busy-spinning.
            async move {
                let micros = duration.as_micros().min(u64::MAX as u128) as u64;
                Timer::after(EmbassyDuration::from_micros(micros)).await;
            }
            .boxed()
        }

        fn page_size() -> usize {
            4096
        }
    }
}

fn log_controller_debug(info: &xhci::ControllerInfo, mask: usize) {
    // Gather a snapshot of the controller MMIO state before handing it to CrabUSB.
    unsafe {
        let cap = info.mmio_base.as_ptr();
        let caplength = read_volatile(cap as *const u8) as u64;
        let hci_version = read_volatile(cap.add(0x02) as *const u16);
        let hcsparams1 = read_volatile(cap.add(0x04) as *const u32);
        let hcsparams2 = read_volatile(cap.add(0x08) as *const u32);
        let hcsparams3 = read_volatile(cap.add(0x0C) as *const u32);
        let hccparams1 = read_volatile(cap.add(0x10) as *const u32);
        let dboff = read_volatile(cap.add(0x14) as *const u32);
        let rtsoff = read_volatile(cap.add(0x18) as *const u32);
        let hccparams2 = read_volatile(cap.add(0x1C) as *const u32);

        let max_slots = (hcsparams1 & 0xFF) as usize;
        let max_intrs = ((hcsparams1 >> 8) & 0x7FF) as usize;
        let max_ports = ((hcsparams1 >> 24) & 0xFF) as usize;
        let max_scratchpads = ((hcsparams2 >> 21) & 0x1F) as usize;
        let context_size_64 = (hccparams1 & (1 << 2)) != 0;

        let op = cap.add(caplength as usize);
        let op32 = op as *const u32;
        let doorbells = cap.add(dboff as usize);
        let runtimes = cap.add(rtsoff as usize);

        let usbcmd = read_volatile(op32.add(0x00 / 4));
        let usbsts = read_volatile(op32.add(0x04 / 4));
        let pagesize = read_volatile(op32.add(0x08 / 4));
        let dnctrl = read_volatile(op32.add(0x14 / 4));

        let crcr_lo = read_volatile(op32.add(0x18 / 4));
        let crcr_hi = if info.supports_64bit {
            read_volatile(op32.add(0x1C / 4))
        } else {
            0
        };
        let crcr = ((crcr_hi as u64) << 32) | crcr_lo as u64;

        let config = read_volatile(op32.add(0x38 / 4));

        let mfindex = read_volatile(runtimes.add(0x20) as *const u32);

        // Interrupter 0 snapshot.
        let ir0_base = runtimes.add(0x20);
        let iman = read_volatile(ir0_base as *const u32);
        let imod = read_volatile(ir0_base.add(0x04) as *const u32);
        let erstsz = read_volatile(ir0_base.add(0x08) as *const u32);
        let erstba_lo = read_volatile(ir0_base.add(0x10) as *const u32);
        let erstba_hi = read_volatile(ir0_base.add(0x14) as *const u32);
        let erstba = ((erstba_hi as u64) << 32) | erstba_lo as u64;
        let erdp_lo = read_volatile(ir0_base.add(0x18) as *const u32);
        let erdp_hi = read_volatile(ir0_base.add(0x1C) as *const u32);
        let erdp = ((erdp_hi as u64) << 32) | erdp_lo as u64;

        debugconf!(
            "usb: xhci snapshot bus={:02X}:{:02X}.{} phys=0x{:X} size=0x{:X} mmio=0x{:X} op=0x{:X} db=0x{:X} rt=0x{:X} mask=0x{:X} 64bit={}\n",
            info.bus,
            info.slot,
            info.function,
            info.bar_phys,
            info.bar_size,
            info.mmio_base.as_ptr() as usize,
            op as usize,
            doorbells as usize,
            runtimes as usize,
            mask,
            info.supports_64bit,
        );

        debugconf!(
            "usb: xhci caps caplen=0x{:X} ver=0x{:04X} hcs1=0x{:X} hcs2=0x{:X} hcs3=0x{:X} hcc1=0x{:X} hcc2=0x{:X} dboff=0x{:X} rtsoff=0x{:X}\n",
            caplength,
            hci_version,
            hcsparams1,
            hcsparams2,
            hcsparams3,
            hccparams1,
            hccparams2,
            dboff,
            rtsoff,
        );

        debugconf!(
            "usb: xhci regs usbcmd=0x{:X} usbsts=0x{:X} pagesize=0x{:X} dnctrl=0x{:X} crcr=0x{:X} config=0x{:X}\n",
            usbcmd,
            usbsts,
            pagesize,
            dnctrl,
            crcr,
            config,
        );

        debugconf!(
            "usb: xhci derived max_slots={} max_intrs={} max_ports={} max_scratchpads={} csz64={} mfindex=0x{:X}\n",
            max_slots,
            max_intrs,
            max_ports,
            max_scratchpads,
            context_size_64,
            mfindex,
        );

        debugconf!(
            "usb: xhci ir0 iman=0x{:X} imod=0x{:X} erstsz=0x{:X} erstba=0x{:X} erdp=0x{:X}\n",
            iman,
            imod,
            erstsz,
            erstba,
            erdp,
        );

        // Dump a few doorbells to see if firmware left anything interesting.
        let db_ptr = doorbells as *const u32;
        let db0 = read_volatile(db_ptr.add(0));
        let db1 = read_volatile(db_ptr.add(1));
        let db2 = read_volatile(db_ptr.add(2));
        let db3 = read_volatile(db_ptr.add(3));
        debugconf!(
            "usb: xhci db[0..4] = [{:X}, {:X}, {:X}, {:X}]\n",
            db0,
            db1,
            db2,
            db3,
        );

        // Dump port status/control registers to understand current link state/ownership.
        let ports_to_dump = max_ports.min(16); // avoid excessive spam on hosts with many ports
        for port in 0..ports_to_dump {
            let off = 0x400 + 0x10 * port;
            let portsc = read_volatile(op.add(off) as *const u32);
            debugconf!(
                "usb: xhci port{} portsc=0x{:X}\n",
                port + 1,
                portsc,
            );
        }

        // Walk extended capabilities (if any) to spot firmware/BIOS ownership, MSI-X, etc.
        let mut ecp = ((hccparams1 >> 16) & 0xFFFF) as usize;
        let mut safety = 0;
        while ecp != 0 && safety < 32 {
            let ecaddr = cap.add(ecp * 4);
            let ec0 = read_volatile(ecaddr as *const u32);
            let ec_id = ec0 & 0xFF;
            let ec_next = ((ec0 >> 8) & 0xFF) as usize;
            debugconf!(
                "usb: xhci extcap id=0x{:X} next=0x{:X} val0=0x{:X} @0x{:X}\n",
                ec_id,
                ec_next,
                ec0,
                ecaddr as usize,
            );

            // For legacy support, dump the control/status dword too.
            if ec_id == 0x1 {
                let ec1 = read_volatile(ecaddr.add(4) as *const u32);
                debugconf!("usb: xhci extcap legacy ctlsts=0x{:X}\n", ec1);
            }

            ecp = ec_next;
            safety += 1;
        }
    }
}

#[embassy_executor::task]
async fn crab_usb_init_task(info: xhci::ControllerInfo) {
    ensure_dma_api_initialized();
    let mask = dma_mask(info.supports_64bit);

    let mut host = USBHost::new_xhci(info.mmio_base, mask);

    debugconf!(
        "usb: starting CrabUSB init bus={:02X}:{:02X}.{} mmio=0x{:X} mask=0x{:X}\n",
        info.bus,
        info.slot,
        info.function,
        info.mmio_base.as_ptr() as usize,
        mask,
    );
    log_controller_debug(&info, mask);

    if let Err(e) = host.init().await {
        debugconf!("usb: CrabUSB init failed: {:?}\n", e);
        return;
    }

    debugconf!("usb: CrabUSB init ok\n");

    *USB_HOST.lock() = Some(host);

    debugconf!(
        "usb: CrabUSB host registered bus={:02X}:{:02X}.{} mmio=0x{:X} mask=0x{:X}\n",
        info.bus,
        info.slot,
        info.function,
        info.mmio_base.as_ptr() as usize,
        mask,
    );
}
