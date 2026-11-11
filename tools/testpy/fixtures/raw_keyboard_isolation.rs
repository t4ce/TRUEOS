#![allow(dead_code)]
extern crate alloc;
use std::cell::{Cell,RefCell};
thread_local! {
    static HOST_ALLOC_DEPTH:Cell<usize>=const{Cell::new(0)};
    static LAST_ALLOC:Cell<(usize,bool)>=const{Cell::new((0,false))};
}
struct DomainTrackingAllocator;
#[global_allocator]static ALLOCATOR:DomainTrackingAllocator=DomainTrackingAllocator;
unsafe impl std::alloc::GlobalAlloc for DomainTrackingAllocator {
    unsafe fn alloc(&self,layout:std::alloc::Layout)->*mut u8{
        let ptr=unsafe{std::alloc::GlobalAlloc::alloc(&std::alloc::System,layout)};
        let host=HOST_ALLOC_DEPTH.try_with(|depth|depth.get()>0).unwrap_or(false);
        let _=LAST_ALLOC.try_with(|last|last.set((ptr as usize,host)));ptr
    }
    unsafe fn dealloc(&self,ptr:*mut u8,layout:std::alloc::Layout){unsafe{std::alloc::GlobalAlloc::dealloc(&std::alloc::System,ptr,layout)}}
}
fn assert_host_scratch(ptr:usize,empty:bool){
    if !empty{assert_eq!(LAST_ALLOC.with(Cell::get),(ptr,true),"raw keyboard scratch must actually allocate inside the strong host scope");}
}
mod input_combo_policy {
    #[derive(Clone,Copy)] pub struct Guest { pub vm:u8, pub run:u64 }
}
#[derive(Clone,Copy,Debug,PartialEq)] enum KeyboardControlPrincipal { Kernel, Vm(u8) }
#[derive(Clone,Copy,Debug,PartialEq)] enum MouseControlPrincipal { Vm(u8) }
#[derive(Clone,Copy,Debug,PartialEq)] enum GamepadControlPrincipal { Vm(u8) }
#[derive(Default)] struct Boundary {
    vm:Option<u8>, hull:bool, run:Option<u64>,
    events:Vec<r::keyboard::TrueosKeyboardOutputEvent>,
    snapshots:Vec<usb2::hid::hut::TrueosHidHutKeyboardState>,
    owners:Vec<(u32,u8)>, pop_seq:u64, pop_calls:usize,
    raw_event_output:usize, raw_snapshot_output:usize,
    host_alloc_depth:usize,
}
thread_local! { static BOUNDARY:RefCell<Boundary>=RefCell::new(Boundary::default()); }
fn boundary<T>(f:impl FnOnce(&mut Boundary)->T)->T { BOUNDARY.with(|s|f(&mut s.borrow_mut())) }
mod allocators {
    pub fn with_host_alloc_domain_strong<T>(f:impl FnOnce()->T)->T{
        crate::boundary(|s|s.host_alloc_depth+=1);
        crate::HOST_ALLOC_DEPTH.with(|depth|depth.set(depth.get()+1));
        let result=f();
        crate::HOST_ALLOC_DEPTH.with(|depth|depth.set(depth.get()-1));
        crate::boundary(|s|s.host_alloc_depth-=1);
        result
    }
}
mod hv {
    pub fn current_guest_execution_context_vm_id()->Option<u8>{super::boundary(|s|s.vm)}
    pub fn current_hull_guest_context_vm_id()->Option<u8>{super::boundary(|s|if s.hull{s.vm}else{None})}
    pub fn vm_run_generation(_:u8)->Option<u64>{super::boundary(|s|s.run)}
}
mod r {
    pub mod services {
        pub mod keyboard_control_service {
            pub fn keyboard_principal(slot:u32)->Option<crate::KeyboardControlPrincipal>{crate::boundary(|s|s.owners.iter().find(|(id,_)|*id==slot).map(|(_,vm)|crate::KeyboardControlPrincipal::Vm(*vm)))}
        }
        pub mod mouse_motion_service { pub fn cursor_principal(_:u32)->Option<crate::MouseControlPrincipal>{None} }
        pub mod gamepad_control_service { pub fn gamepad_principal(_:u32)->Option<crate::GamepadControlPrincipal>{None} }
    }
    pub mod io { pub mod input_cabi { pub(crate) use crate::{guest_hid_hut_read_keyboards,read_keyboard_snapshot_for_caller}; } }
    pub mod keyboard {
        @EVENT@
        pub fn read_output_events_since(seq:u64,out:&mut[TrueosKeyboardOutputEvent])->(u64,u32,usize){
            crate::boundary(|s|{
                assert!(s.vm.is_none()||s.host_alloc_depth>0,"unfiltered guest keyboard records require a strong host allocation domain");
                if s.vm.is_some(){crate::assert_host_scratch(out.as_ptr() as usize,out.is_empty());}
                s.raw_event_output=out.as_ptr() as usize;
                let start=(seq as usize).min(s.events.len());
                let n=out.len().min(s.events.len()-start);
                out[..n].copy_from_slice(&s.events[start..start+n]);
                (seq+n as u64,0,n)
            })
        }
        pub fn pop_output_event()->Option<TrueosKeyboardOutputEvent>{crate::boundary(|s|{
            s.pop_calls+=1;let event=s.events.get(s.pop_seq as usize).copied();if event.is_some(){s.pop_seq+=1;}event
        })}
    }
}
mod usb2 { pub mod hid { pub mod hut {
    const HID_HUT_SOURCE_TAG_MAX:usize=32;
    @STATE@
    pub type TrueosHidHutMouseState=();
    pub type TrueosHidHutTabletState=();
    pub fn mice_snapshot()->Vec<()>{Vec::new()}
    pub fn tablets_snapshot()->Vec<()>{Vec::new()}
    pub fn combos_snapshot()->Vec<()>{Vec::new()}
    pub fn read_mice_snapshot(_:&mut[()])->usize{0}
    pub fn read_tablets_snapshot(_:&mut[()])->usize{0}
    pub fn keyboards_snapshot()->Vec<TrueosHidHutKeyboardState>{crate::boundary(|s|s.snapshots.clone())}
    pub fn read_keyboards_snapshot(out:&mut[TrueosHidHutKeyboardState])->usize{crate::boundary(|s|{
        assert!(s.vm.is_none()||s.host_alloc_depth>0,"unfiltered guest HUT state requires a strong host allocation domain");
        if s.vm.is_some(){crate::assert_host_scratch(out.as_ptr() as usize,out.is_empty());}
        s.raw_snapshot_output=out.as_ptr() as usize;let n=out.len().min(s.snapshots.len());out[..n].copy_from_slice(&s.snapshots[..n]);n
    })}
} } }
use usb2::hid::hut;
fn input_combo_info(_:&())->(){()}
mod input_wire {
    use crate::r::keyboard::TrueosKeyboardOutputEvent;
    use crate::usb2::hid::hut::TrueosHidHutKeyboardState;
    pub const HUT_SNAPSHOT_LIMIT:u32=32;
    pub const HUT_SNAPSHOT_KEYBOARDS:u32=1;
    pub const HUT_SNAPSHOT_COMBOS:u32=2;
    pub const HUT_SNAPSHOT_MICE:u32=3;
    pub const HUT_SNAPSHOT_TABLETS:u32=4;
    pub const HUT_KEYBOARD_LEN:usize=96;
    pub const KEYBOARD_OUTPUT_LEN:usize=44;
    pub const HUT_MOUSE_LEN:usize=72;
    pub const HUT_TABLET_LEN:usize=80;
    pub const INPUT_COMBO_LEN:usize=88;
    pub fn hut_mouse(_:&mut[u8],_:&())->bool{true}
    pub fn hut_tablet(_:&mut[u8],_:&())->bool{true}
    pub fn input_combo(_:&mut[u8],_:&())->bool{true}
    @WIRE@
}
mod trueos_vm { pub mod vmcall {
    pub const PAYLOAD_CAP:usize=4096;
    pub const STATUS_OK:u32=0;
    pub const OP_BP_INPUT_HUT_SNAPSHOT_V1:u32=1;
    pub const OP_BP_INPUT_KEYBOARD_OUTPUT_POP:u32=2;
    pub const OP_BP_INPUT_KEYBOARD_OUTPUT_SINCE:u32=3;
    pub fn call_with_payload(op:u32,a:u64,b:u64,_:&[u8],payload:&mut[u8])->(u32,u64){
        let(status,data,_)=call_with_payload_reported_len(op,a,b,&[],payload);(status,data)
    }
    pub fn call_with_payload_reported_len(op:u32,a:u64,b:u64,_:&[u8],payload:&mut[u8])->(u32,u64,usize){
        match op {
            OP_BP_INPUT_HUT_SNAPSHOT_V1=>match crate::host_input_hut_snapshot_v1(a as u32,b as u32,payload){Ok((n,bytes))=>(0,n as u64,bytes),Err(_)=>(1,0,0)},
            OP_BP_INPUT_KEYBOARD_OUTPUT_POP=>{let(rc,bytes)=crate::host_input_pop_keyboard_output(payload);(0,rc as u64,bytes)},
            OP_BP_INPUT_KEYBOARD_OUTPUT_SINCE=>{let(n,bytes)=crate::host_input_keyboard_output_since(a,b as u32,payload);(0,n as u64,bytes)},
            _=>panic!("unexpected opcode"),
        }
    }
} }
@PRODUCTION@
@HUT_GETTER@

#[cfg(test)] mod tests {
    use super::*;
    use r::keyboard::TrueosKeyboardOutputEvent as Event;
    use hut::TrueosHidHutKeyboardState as State;
    fn setup(vm:Option<u8>,hull:bool){boundary(|s|{
        *s=Boundary{vm,hull,run:Some(9),owners:vec![(11,1),(22,2)],..Default::default()};
        s.events=vec![event(7,77,71),event(0,22,22),event(0,11,11),event(0,11,12)];
        s.snapshots=vec![state(7,77),state(0,22),state(0,11)];
    })}
    fn event(controller:u32,slot:u32,codepoint:u32)->Event{Event{controller_id:controller,slot_id:slot,codepoint,utf8:[codepoint as u8;4],modifiers:0xff,..Default::default()}}
    fn state(controller:u32,slot:u32)->State{State{controller_id:controller,slot_id:slot,combo_id:slot+100,modifiers:0xff,keys:[1;6],ascii:[2;6],key_down_bits:[u32::MAX;8],source_tag:[3;32],source_kind:1,virtual_device:1,source_tag_len:32,..Default::default()}}
    fn read(cap:usize)->(Vec<Event>,u64,u32){
        let mut out=vec![Event::default();cap];let mut next=0;let mut dropped=0;
        let n=unsafe{trueos_cabi_input_read_keyboard_output_since(0,out.as_mut_ptr(),cap as u32,&mut next,&mut dropped)};
        assert!(n as usize<=cap);out.truncate(n as usize);(out,next,dropped)
    }
    #[test] fn direct_guest_context_only_reads_owned_virtual_keys(){
        setup(Some(1),false);let mut out=[Event::default();8];let mut next=0;let mut dropped=0;
        let n=unsafe{trueos_cabi_input_read_keyboard_output_since(0,out.as_mut_ptr(),8,&mut next,&mut dropped)};
        assert_eq!((n,next,dropped),(2,4,0));assert_eq!(out[0].codepoint,11);assert_eq!(out[1].codepoint,12);
        assert_ne!(boundary(|s|s.raw_event_output),out.as_ptr() as usize,"hidden records must never be written to guest storage");
        assert_eq!(boundary(|s|s.host_alloc_depth),0,"host allocation scope must be restored after direct CABI delivery");
    }
    #[test] fn hull_vmcall_keyboard_read_uses_same_filter(){
        setup(Some(1),true);let(out,next,dropped)=read(8);
        assert_eq!((out.len(),next,dropped),(2,4,0));assert_eq!(out[0].slot_id,11);assert_eq!(out[1].codepoint,12);
    }
    #[test] fn foreign_only_batch_advances_cursor_and_does_not_starve_owned_later_batch(){
        setup(Some(1),false);let(out,next,_)=read(2);assert!(out.is_empty());assert_eq!(next,2);
        let mut out=[Event::default();2];let mut next_seq=next;let mut dropped=0;
        let n=unsafe{trueos_cabi_input_read_keyboard_output_since(next,out.as_mut_ptr(),2,&mut next_seq,&mut dropped)};
        assert_eq!((n,next_seq),(2,4));assert_eq!(out[0].codepoint,11);
    }
    #[test] fn guest_pop_never_consumes_shared_queue_in_either_path(){
        for hull in [false,true]{setup(Some(1),hull);let mut event=Event::default();
            assert_eq!(unsafe{trueos_cabi_input_pop_keyboard_output(&mut event)},1);
            assert_eq!(boundary(|s|s.pop_calls),0);assert_eq!(boundary(|s|s.pop_seq),0);
        }
    }
    fn check_states(hull:bool){
        setup(Some(1),hull);assert_eq!(unsafe{trueos_cabi_hid_hut_read_keyboards(core::ptr::null_mut(),0)},3);
        let mut out=[State::default();4];let n=unsafe{trueos_cabi_hid_hut_read_keyboards(out.as_mut_ptr(),4)};assert_eq!(n,3);
        for(index,key)in out[..3].iter().enumerate(){
            let original=boundary(|s|s.snapshots[index]);
            assert_eq!((key.controller_id,key.slot_id,key.ep_target,key.combo_id,key.source_kind,key.source_tag,key.virtual_device,key.source_tag_len),
                (original.controller_id,original.slot_id,original.ep_target,original.combo_id,original.source_kind,original.source_tag,original.virtual_device,original.source_tag_len));
            if key.slot_id==11{assert_eq!(key.modifiers,0xff);assert_eq!(key.key_down_bits,[u32::MAX;8]);}
            else{assert_eq!(key.modifiers,0);assert_eq!(key.keys,[0;6]);assert_eq!(key.ascii,[0;6]);assert_eq!(key.key_down_bits,[0;8]);}
        }
        assert_ne!(boundary(|s|s.raw_snapshot_output),out.as_ptr() as usize,"unscrubbed snapshots must never reach guest storage");
        assert_eq!(boundary(|s|s.host_alloc_depth),0,"host allocation scope must be restored after HUT delivery");
    }
    #[test]fn direct_guest_snapshot_preserves_inventory_but_scrubs_foreign_live_state(){check_states(false)}
    #[test]fn hull_snapshot_preserves_inventory_but_scrubs_foreign_live_state(){check_states(true)}
    #[test]fn native_keyboard_diagnostics_keep_global_access(){
        setup(None,false);let(out,next,_)=read(8);assert_eq!((out.len(),next),(4,4));assert_eq!(out[0].codepoint,71);
        let mut key=Event::default();assert_eq!(unsafe{trueos_cabi_input_pop_keyboard_output(&mut key)},0);assert_eq!(key.codepoint,71);
        let mut states=[State::default();4];assert_eq!(unsafe{trueos_cabi_hid_hut_read_keyboards(states.as_mut_ptr(),4)},3);assert_eq!(states[0].modifiers,0xff);assert_eq!(states[1].key_down_bits,[u32::MAX;8]);
    }
    #[test]fn invalid_guest_run_never_falls_back_to_native_keyboard_access(){
        for hull in [false,true]{setup(Some(1),hull);boundary(|s|s.run=None);let(out,next,_)=read(8);assert!(out.is_empty());assert_eq!(next,4);
            let mut key=Event::default();assert_eq!(unsafe{trueos_cabi_input_pop_keyboard_output(&mut key)},1);assert_eq!(boundary(|s|s.pop_calls),0);
            let mut states=[State::default();4];assert_eq!(unsafe{trueos_cabi_hid_hut_read_keyboards(states.as_mut_ptr(),4)},3);assert!(states[..3].iter().all(|key|key.modifiers==0&&key.key_down_bits==[0;8]));
        }
    }
    #[test]fn spoofed_controller_or_endpoint_cannot_borrow_owned_station_slot(){
        setup(Some(1),false);boundary(|s|{s.events=vec![event(1,11,70),Event{ep_target:1,..event(0,11,71)},event(0,11,72)];});
        let(out,next,_)=read(8);assert_eq!((out.len(),next),(1,3));assert_eq!(out[0].codepoint,72);
    }
}
