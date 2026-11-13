#![allow(dead_code, unused_imports, unused_variables)]
extern crate self as spin;
extern crate self as trueos_time;
use std::{cell::RefCell, collections::BTreeMap};

pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value:T)->Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self)->std::sync::MutexGuard<'_,T> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }
    pub fn try_lock(&self)->Option<std::sync::MutexGuard<'_,T>> { self.0.try_lock().ok() }
}
pub struct Timer;
impl Timer {pub fn after_millis(_:u64)->Self {Self}}
impl core::future::Future for Timer {
    type Output=();
    fn poll(self:core::pin::Pin<&mut Self>,_:&mut core::task::Context<'_>)->core::task::Poll<()> {
        core::task::Poll::Pending
    }
}
#[macro_export] macro_rules! log_info {($($tt:tt)*)=>{{}};}
#[macro_export] macro_rules! log_warn {($($tt:tt)*)=>{{}};}
#[macro_export] macro_rules! log_error {($($tt:tt)*)=>{{}};}

struct Model {
    registers:BTreeMap<usize,u32>, writes:Vec<(usize,u32)>, notifications:Vec<usize>,
    allocations:BTreeMap<usize,usize>, maps:usize, frees:usize, native:bool,
    fail_enable:bool, fail_interrupt:bool, complete_on_stop:bool, fail_disable:bool,
    irq_enabled:bool,
}
impl Default for Model {
    fn default()->Self {
        let mut registers=BTreeMap::new();
        registers.insert(0x70008,1<<30); // Wired A is running.
        registers.insert(0x6001c,((320-1)<<16)|(200-1));
        registers.insert(0x70034,0x004080c0);
        registers.insert(0x70080,0x27);
        registers.insert(0x700ac,0x02000000);
        registers.insert(0x7017c,(2039<<16)|2032);
        for slot in 0..5 {
            let base=0x70180+slot*0x100;
            registers.insert(base,1<<31);
            registers.insert(base+0x1c,0x02000000+slot as u32*0x100000);
            let start=1024+slot as u32*205;
            let end=if slot==4 {2031} else {start+204};
            registers.insert(base+0xfc,(end<<16)|start);
        }
        Self{registers,writes:Vec::new(),notifications:Vec::new(),allocations:BTreeMap::new(),
             maps:0,frees:0,native:false,fail_enable:false,fail_interrupt:false,
             complete_on_stop:true,fail_disable:false,irq_enabled:false}
    }
}
thread_local! { static MODEL:RefCell<Model>=RefCell::new(Model::default()); }
fn model<T>(f:impl FnOnce(&mut Model)->T)->T {MODEL.with(|state| f(&mut state.borrow_mut()))}
fn reg(address:usize)->u32 {model(|state| *state.registers.get(&address).unwrap_or(&0))}
fn set(address:usize,value:u32) {model(|state|{state.registers.insert(address,value);});}

mod ui4 {
    pub fn notify_chroma_pipe_route_changed(pipe:usize) {
        crate::model(|state| state.notifications.push(pipe));
    }
}
mod dma {
    pub fn dealloc(pointer:*mut u8,bytes:usize) {
        assert_eq!(crate::model(|state|{state.frees+=1;state.allocations.remove(&(pointer as usize))}),Some(bytes));
        unsafe {drop(Box::from_raw(core::ptr::slice_from_raw_parts_mut(pointer,bytes)));}
    }
}
mod intel {
    pub const SPIRIT_CURSOR_DBUF_S1_START:u16=1008;
    #[derive(Clone,Copy)] pub struct Dev {pub device_id:u16}
    pub fn claimed_device()->Option<Dev>{Some(Dev{device_id:0x4680})}
    pub fn mmio_read(_:Dev,address:usize)->u32 {crate::reg(address)}
    pub fn mmio_write(_:Dev,address:usize,value:u32) {
        // Completion only acknowledges WD's sticky status under STATE; it
        // cannot power down B or change any pipe color state.
        assert!(address==0x6e568 || crate::display::DISPLAY_COLOR_ROUTE_LOCK.try_lock().is_none(),"route lock absent at {address:x}");
        crate::model(|state| {
            state.writes.push((address,value));
            let old=*state.registers.get(&address).unwrap_or(&0);
            let effective=match address {
                0x45404 => value | ((value & 0xaa)>>1),
                0x45008 | 0x44fe8 => if value&(1<<31)!=0 {value|(1<<30)} else {value&!(1<<30)},
                0x7e008 => {
                    if value&(1<<31)!=0 {if state.fail_enable {value} else {value|(1<<30)}}
                    else if state.fail_disable {1<<30} else {value}
                }
                0x6e568 => old & !value,
                _ => value,
            };
            state.registers.insert(address,effective);
            if address==0x6e400 && value&(1<<28)!=0 && state.complete_on_stop {
                state.registers.insert(0x6e568,1<<31);
            }
        });
    }
    pub fn alloc_ggtt_backing(bytes:usize,_:usize)->Option<(u64,*mut u8)> {
        let block=vec![0u8;bytes].into_boxed_slice();
        let pointer=Box::into_raw(block).cast::<u8>();
        crate::model(|state|{state.allocations.insert(pointer as usize,bytes);});
        Some((pointer as u64,pointer))
    }
    pub fn dma_flush(_: *mut u8,_:usize){}
    pub fn map_display_scanout_ggtt(_:Dev,_:u64,_:usize,_:u64)->bool {
        crate::model(|state|state.maps+=1);true
    }
    pub fn unmap_display_scanout_ggtt(_:Dev,_:usize,_:u64)->bool {
        crate::model(|state|state.maps-=1);true
    }
    pub fn ggtt_invalidate(_:Dev){}
    pub mod interrupts {
        pub fn enable_wd(_:super::Dev)->bool {
            crate::model(|state|{state.irq_enabled=!state.fail_interrupt;state.irq_enabled})
        }
        pub fn disable_wd(_:super::Dev){crate::model(|state|state.irq_enabled=false);}
        pub fn wd_generation()->u64{0}
        pub fn wd_route_ready()->bool{true}
        pub async fn wait_wd_after<T>(_:u64,_:core::pin::Pin<&mut T>)->bool{false}
    }
}
mod display {
    use crate::Mutex;
    @REGISTERS@
    pub(crate) static DISPLAY_COLOR_ROUTE_LOCK:Mutex<()>=Mutex::new(());
    static UI4_PLANE_SURFACE_FLIP_BATCH:Mutex<()>=Mutex::new(());
    const UNIVERSAL_PLANE_SLOTS:usize=5;
    #[derive(Clone,Copy)] struct PipeInfo {slot:usize,pipe_src_off:usize}
    impl PipeInfo {const fn plane(self,slot:usize)->PlaneId{PlaneId{pipe:self,slot}}}
    #[derive(Clone,Copy)] struct PlaneId {pipe:PipeInfo,slot:usize}
    impl PlaneId {
        fn base(self)->usize{UNI_PLANE_BASE+self.pipe.slot*UNI_PLANE_PIPE_STRIDE+self.slot*UNI_PLANE_SLOT_STRIDE}
        fn ctl(self)->usize{self.base()+UNI_PLANE_CTL_OFF}
        fn surf(self)->usize{self.base()+UNI_PLANE_SURF_OFF}
        fn buf_cfg(self)->usize{self.base()+UNI_PLANE_BUF_CFG_OFF}
    }
    const PIPES:[PipeInfo;4]=[
        PipeInfo{slot:0,pipe_src_off:PIPE_A_SRC},PipeInfo{slot:1,pipe_src_off:PIPE_B_SRC},
        PipeInfo{slot:2,pipe_src_off:PIPE_C_SRC},PipeInfo{slot:3,pipe_src_off:PIPE_D_SRC},
    ];
    fn native_ui4_for_pipe(_:PipeInfo)->bool{crate::model(|state|state.native)}
    @HELPERS@
    mod captured {
        @MIRROR@

        fn reset() {
            let old=crate::model(|state|core::mem::replace(state,crate::Model::default()).allocations);
            *STATE.lock()=WdCaptureState::new();
            for (pointer,bytes) in old {
                unsafe {drop(Box::from_raw(core::ptr::slice_from_raw_parts_mut(pointer as *mut u8,bytes)));}
            }
        }
        fn assert_no_foreign_color_or_pipe_writes() {
            crate::model(|state|for &(address,_) in &state.writes {
                assert!(!(0x72000..0x74000).contains(&address),"C/D pipe write {address:x}");
                assert!(!(0x49200..0x49400).contains(&address),"C/D color write {address:x}");
                assert!(!(0x69100..0x6a000).contains(&address),"C/D scaler write {address:x}");
                assert!(![0x490a4,0x490a8,0x490ac,0x491a4,0x491a8,0x491ac,0x4a400,0x4a404,0x4a480].contains(&address),"LUT/gamma write {address:x}");
                assert_ne!(address,0x71008,"ordinary B transcoder must stay disabled");
                assert_ne!(address,0x70008,"ordinary A transcoder must stay untouched");
            });
        }
        #[test]
        fn start_capture_uses_b_ports_and_stop_releases_only_its_route() {
            reset();
            let frame=start_ui4_wd_xyuv8888_capture().unwrap();
            assert_eq!((frame.width,frame.height),(320,200));
            assert_eq!((crate::reg(WD0_FUNC_CTL)>>12)&7,5);
            assert_eq!(MIRROR_PIPE_SLOT,1);
            assert_eq!(crate::reg(0x7117c),(1023<<16)|1016);
            assert_eq!(crate::reg(0x71180+UNI_PLANE_BUF_CFG_OFF),204<<16);
            assert_eq!(crate::reg(0x71180+UNI_PLANE_SURF_OFF),0x02000000);
            assert_eq!(crate::reg(0x49128)&PIPE_OUTPUT_CSC_ENABLE,PIPE_OUTPUT_CSC_ENABLE);
            assert_eq!(crate::model(|s|s.notifications.clone()),vec![1]);
            assert!(crate::model(|s|s.irq_enabled));
            assert_no_foreign_color_or_pipe_writes();
            stop_ui4_wd_xyuv8888_capture().unwrap();
            assert_eq!(crate::model(|s|(s.maps,s.allocations.len(),s.notifications.clone())),(0,0,vec![1,1]));
            assert_eq!(crate::reg(0x70008),1<<30);
            assert!(!crate::model(|s|s.irq_enabled));
            assert!(DISPLAY_COLOR_ROUTE_LOCK.try_lock().is_some());
            assert_no_foreign_color_or_pipe_writes();
        }
        #[test]
        fn output_csc_is_after_the_lut_and_restores_the_previous_b_registers() {
            reset();
            let addresses=[0x71030,0x49128,0x4ac80,0x49150,0x49154,0x49158,0x4915c,0x49160,0x49164,
                           0x49168,0x4916c,0x49170,0x49174,0x49178,0x4917c];
            for (index,address) in addresses.iter().enumerate(){crate::set(*address,0x12340000|index as u32);}
            crate::set(0x4ac80,0xc1234567);
            let before=addresses.map(crate::reg);
            start_ui4_wd_xyuv8888_capture().unwrap();
            assert_eq!(crate::reg(0x49150),0x1e089cc0);
            assert_eq!(crate::reg(0x49174),0x800);
            assert_eq!(crate::reg(0x4ac80),0x01234567);
            stop_ui4_wd_xyuv8888_capture().unwrap();
            assert_eq!(addresses.map(crate::reg),before);
            assert_no_foreign_color_or_pipe_writes();
        }
        #[test]
        fn occupied_b_or_wd_and_source_buffer_overlap_fail_before_mutation() {
            for (address,value) in [(0x71008,1<<30),(0x61400,1<<31),(0x7e008,1<<30),(0x6e400,1<<31),
                                    (0x70180+UNI_PLANE_BUF_CFG_OFF,204<<16),(0x7017c,(1023<<16)|1016)] {
                reset();crate::set(address,value);
                assert_eq!(start_ui4_wd_xyuv8888_capture().unwrap_err(),WdCaptureError::MirrorPipeBusy);
                assert!(crate::model(|s|s.writes.is_empty()&&s.allocations.is_empty()&&s.maps==0));
            }
        }
        #[test]
        fn disabled_source_allocations_do_not_block_a_disjoint_capture() {
            reset();
            crate::set(0x70180,0);crate::set(0x70180+UNI_PLANE_BUF_CFG_OFF,204<<16);
            crate::set(0x70080,1<<31);crate::set(0x7017c,(1023<<16)|1016);
            start_ui4_wd_xyuv8888_capture().unwrap();
            stop_ui4_wd_xyuv8888_capture().unwrap();
        }
        #[test]
        fn native_tgl_dbuf_contract_still_rejects_before_any_writes() {
            reset();crate::model(|state|state.native=true);
            assert_eq!(start_ui4_wd_xyuv8888_capture().unwrap_err(),WdCaptureError::UnsupportedDevice);
            assert!(crate::model(|s|s.writes.is_empty()&&s.allocations.is_empty()));
        }
        #[test]
        fn start_failure_cleanup_drops_route_lock_before_teardown() {
            reset();crate::model(|state|state.fail_interrupt=true);
            assert_eq!(start_ui4_wd_xyuv8888_capture().unwrap_err(),WdCaptureError::InterruptUnavailable);
            assert_eq!(crate::model(|s|(s.maps,s.allocations.len(),s.notifications.clone())),(0,0,vec![1]));
            assert!(!STATE.lock().running);
            assert!(DISPLAY_COLOR_ROUTE_LOCK.try_lock().is_some());
        }
        #[test]
        fn enable_timeout_cleanup_also_drops_route_lock_and_restores_b_color() {
            reset();crate::model(|state|state.fail_enable=true);
            crate::set(0x4ac80,0xc1234567);
            assert_eq!(start_ui4_wd_xyuv8888_capture().unwrap_err(),WdCaptureError::WdEnableTimeout);
            assert_eq!(crate::model(|s|(s.maps,s.allocations.len(),s.notifications.clone())),(0,0,vec![1]));
            assert_eq!(crate::reg(0x4ac80),0xc1234567);
            assert!(!STATE.lock().running);
            assert!(DISPLAY_COLOR_ROUTE_LOCK.try_lock().is_some());
        }
        #[test]
        fn mirrored_planes_and_cursor_keep_internal_color_without_stale_b_pipe_palettes() {
            reset();
            let plane_flags=(1<<30)|(1<<23)|(1<<21)|(1<<20)|17;
            let cursor_flags=(1<<26)|(1<<24)|(1<<18)|(1<<16)|0x27;
            for slot in 0..5 {crate::set(0x70180+slot*0x100+UNI_PLANE_COLOR_CTL_OFF,plane_flags);}
            crate::set(0x70080,cursor_flags);
            start_ui4_wd_xyuv8888_capture().unwrap();
            for slot in 0..5 {
                assert_eq!(crate::reg(0x71180+slot*0x100+UNI_PLANE_COLOR_CTL_OFF),plane_flags&!((1<<30)|(1<<23)));
            }
            assert_eq!(crate::reg(0x71080),cursor_flags&!((1<<26)|(1<<24)|(1<<18)));
            stop_ui4_wd_xyuv8888_capture().unwrap();
        }
        #[test]
        fn capture_frames_preserve_chromatic_b_lut_and_do_not_reprogram_output_grade() {
            reset();
            let lut_registers=[0x491a4,0x491a8,0x491ac];
            for (index,address) in lut_registers.iter().enumerate(){crate::set(*address,0x80123400|index as u32);}
            let before=lut_registers.map(crate::reg);
            start_ui4_wd_xyuv8888_capture().unwrap();
            start_ui4_wd_xyuv8888_capture().unwrap(); // Resident reuse is read-only.
            crate::model(|state|state.writes.clear());
            begin_ui4_wd_xyuv8888_capture().unwrap();
            assert_eq!(lut_registers.map(crate::reg),before);
            crate::model(|state|assert!(state.writes.iter().all(|&(address,_)|
                !(0x49100..0x49200).contains(&address) && address!=0x4ac80)));
            crate::set(WD0_FRAME_STATUS,WD_FRAME_COMPLETE);
            assert!(matches!(poll_ui4_wd_xyuv8888_capture(),WdCapturePoll::Complete(_)));
            stop_ui4_wd_xyuv8888_capture().unwrap();
            assert_eq!(lut_registers.map(crate::reg),before);
            assert_eq!(crate::model(|s|s.notifications.clone()),vec![1,1]);
            assert_no_foreign_color_or_pipe_writes();
        }
        #[test]
        fn capture_rechecks_a_buffers_and_pins_completed_source_until_reader_retires() {
            reset();let frame=start_ui4_wd_xyuv8888_capture().unwrap();
            crate::set(0x70180+UNI_PLANE_BUF_CFG_OFF,204<<16);
            crate::model(|state|state.writes.clear());
            assert_eq!(begin_ui4_wd_xyuv8888_capture().unwrap_err(),WdCaptureError::MirrorPipeBusy);
            assert!(crate::model(|state|state.writes.is_empty()));
            crate::set(0x70180+UNI_PLANE_BUF_CFG_OFF,(1228<<16)|1024);
            let sequence=begin_ui4_wd_xyuv8888_capture().unwrap();
            crate::set(WD0_FRAME_STATUS,WD_FRAME_COMPLETE);
            assert!(matches!(poll_ui4_wd_xyuv8888_capture(),WdCapturePoll::Complete(_)));
            let lease=pin_ui4_wd_frame_for_copy(frame.phys,sequence).unwrap();
            assert_eq!(begin_ui4_wd_xyuv8888_capture().unwrap_err(),WdCaptureError::ConsumerBusy);
            assert_eq!(stop_ui4_wd_xyuv8888_capture().unwrap_err(),WdCaptureError::ConsumerBusy);
            drop(lease);
            stop_ui4_wd_xyuv8888_capture().unwrap();
        }
        #[test]
        fn ambiguous_stop_keeps_target_and_b_powered_without_false_route_off_notification() {
            reset();start_ui4_wd_xyuv8888_capture().unwrap();
            begin_ui4_wd_xyuv8888_capture().unwrap();
            crate::model(|state|state.complete_on_stop=false);
            assert_eq!(stop_ui4_wd_xyuv8888_capture().unwrap_err(),WdCaptureError::WdDisableTimeout);
            assert!(STATE.lock().quarantined);
            assert_eq!(crate::model(|state|(state.maps,state.allocations.len(),state.notifications.clone())),(1,1,vec![1]));
            assert_eq!(start_ui4_wd_xyuv8888_capture().unwrap_err(),WdCaptureError::Quarantined);
        }
        #[test]
        fn unretired_transcoder_keeps_mapping_and_does_not_restore_color_or_power() {
            reset();crate::set(0x4ac80,0xc1234567);
            start_ui4_wd_xyuv8888_capture().unwrap();
            crate::model(|state|{state.fail_disable=true;state.writes.clear();});
            assert_eq!(stop_ui4_wd_xyuv8888_capture().unwrap_err(),WdCaptureError::WdDisableTimeout);
            assert!(STATE.lock().quarantined);
            assert_eq!(crate::reg(0x4ac80),0x01234567);
            assert_eq!(crate::model(|state|(state.maps,state.allocations.len(),state.notifications.clone())),(1,1,vec![1]));
            crate::model(|state|assert!(state.writes.iter().all(|&(address,_)|
                ![0x4ac80,0x49128,0x45008,0x45404].contains(&address))));
        }
    }
}
