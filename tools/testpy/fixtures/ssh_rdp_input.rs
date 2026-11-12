#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_crypto;
use std::cell::RefCell;
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct AccountId(u64);
#[derive(Default)] struct World {
    next: u64, sessions: Vec<shell3::tui::RemoteInputAuthority>,
    keyboards: Vec<(u64,u32,u8,[u8;6])>, cursors: Vec<(u64,u32,i32,i32,u32)>,
    combos: Vec<u32>, mouse_reports: Vec<(i32,i32,u32,i16,Option<(i16,i16)>)>,
    snapped: bool, fail_cursor: bool, fail_combo: bool, released_keys: usize, released_cursors: usize,
}
thread_local! { static WORLD: RefCell<World> = RefCell::new(World::default()); }
fn world<R>(f: impl FnOnce(&mut World)->R)->R { WORLD.with(|w| f(&mut w.borrow_mut())) }
fn next()->u64 {world(|w| {w.next+=1;w.next})}
mod shell3 {
    pub mod tui {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct RemoteInputAuthority { pub account:crate::AccountId, session:u64 }
        impl RemoteInputAuthority {pub fn session_id(self)->Option<u64>{Some(self.session)}}
        pub fn open_rdp_input_session(account:crate::AccountId)->RemoteInputAuthority {
            let authority=RemoteInputAuthority{account,session:crate::next()};crate::world(|w| w.sessions.push(authority));authority
        }
        pub fn close_rdp_input_session(session:u64){crate::world(|w|w.sessions.retain(|s|s.session!=session));}
        pub fn remote_input_authority_valid(authority:RemoteInputAuthority)->bool{crate::world(|w|w.sessions.contains(&authority))}
    }
    #[path="@RDP@"] pub mod rdp;
}
mod r {
    pub mod cursor {pub const HID_KIND_MOUSE:u8=1;}
    pub mod services {
        pub mod keyboard_control_service {
            #[derive(Clone,Copy)] pub enum KeyboardControlPrincipal {Kernel}
            #[derive(Clone,Copy)] pub struct KeyboardControlDevice{pub handle:u64,pub slot_id:u32}
            pub fn request_keyboard(_:KeyboardControlPrincipal,_:&str)->Result<KeyboardControlDevice,()>{
                let id=crate::next();let device=KeyboardControlDevice{handle:id,slot_id:id as u32};
                crate::world(|w|w.keyboards.push((id,id as u32,0,[0;6])));Ok(device)
            }
            pub fn release_keyboard(_:KeyboardControlPrincipal,handle:u64)->Result<(),()>{
                crate::world(|w|{w.keyboards.retain(|k|k.0!=handle);w.released_keys+=1;});Ok(())
            }
            pub fn submit_report(_:KeyboardControlPrincipal,handle:u64,modifiers:u8,keys:[u8;6])->Result<(),()>{
                if keys.iter().any(|k|(1..=3).contains(k)){return Err(());}
                crate::world(|w|{let k=w.keyboards.iter_mut().find(|k|k.0==handle).ok_or(())?;k.2=modifiers;k.3=keys;Ok(())})
            }
        }
        pub mod mouse_motion_service {
            #[derive(Clone,Copy)] pub enum MouseControlPrincipal {Kernel}
            #[derive(Clone,Copy)] pub struct MouseControlCursor{pub handle:u64,pub slot_id:u32}
            pub fn request_cursor(_:MouseControlPrincipal,_:&str,_:Option<()>)->Result<MouseControlCursor,()>{
                if crate::world(|w|w.fail_cursor){return Err(());}
                let id=crate::next();crate::world(|w|w.cursors.push((id,id as u32,0,0,0)));Ok(MouseControlCursor{handle:id,slot_id:id as u32})
            }
            pub fn release_cursor(_:MouseControlPrincipal,handle:u64)->Result<(),()>{crate::world(|w|{w.cursors.retain(|c|c.0!=handle);w.released_cursors+=1;});Ok(())}
            pub fn submit_remote_report(_:MouseControlPrincipal,handle:u64,x:i32,y:i32,buttons:u32,wheel:i16,relative:Option<(i16,i16)>)->Result<(),()>{
                crate::world(|w|{let c=w.cursors.iter_mut().find(|c|c.0==handle).ok_or(())?;c.2=x;c.3=y;c.4=buttons;w.mouse_reports.push((x,y,buttons,wheel,relative));Ok(())})
            }
        }
    }
}
mod usb2 {pub mod hid {pub mod hut {
    #[derive(Clone,Copy,Debug,Eq,PartialEq)] pub enum HidDeviceKind{Mouse,Keyboard,Tablet,Gamepad}
    pub enum HidSourceKind{Remote}
    pub struct InputCombo{pub combo_id:u32}
    pub fn request_combo(_:HidSourceKind,_:&str,_:Option<u8>)->Option<InputCombo>{
        if crate::world(|w|w.fail_combo){return None;}
        let id=crate::next() as u32;crate::world(|w|w.combos.push(id));Some(InputCombo{combo_id:id})
    }
    pub fn bind_combo_keyboard(_:u32,_:u32,_:u32,_:u32)->bool{true}
    pub fn bind_combo_mouse(_:u32,_:u32,_:u32,_:u32)->bool{true}
    pub fn remove_combo(id:u32)->bool{crate::world(|w|w.combos.retain(|c|*c!=id));true}
}}}
mod ui4 {
    #[derive(Clone,Copy)] pub struct Ui4CursorSource{pub controller_id:u32,pub slot_id:u32,pub ep_target:u32,pub hid_kind:u8}
    pub struct Frame{pub owner:u8,pub window:u32}
    pub fn output_dimensions()->Option<(u32,u32)>{Some((320,200))}
    pub fn center_snapped_frame_for_source(_:Ui4CursorSource)->Option<Frame>{crate::world(|w|w.snapped.then_some(Frame{owner:1,window:2}))}
    pub fn set_window_center_snapped_mouse(_:u8,_:u32,enabled:bool)->Result<(),()>{crate::world(|w|w.snapped=enabled);Ok(())}
}
mod raw_report_policy {
    #[derive(Clone,Copy,PartialEq,Eq)] enum KeyboardControlPrincipal {Kernel,Vm(u8)}
    #[derive(Debug,PartialEq,Eq)] enum KeyboardControlError {Invalid,Denied}
    struct Record {commands:Vec<u8>,active:Option<u8>,modifiers:u8,keys:[u8;6],device_seq:u32}
    struct Station {keyboards:Vec<Record>}
    impl Station {
        fn index(&self,principal:KeyboardControlPrincipal,handle:u64)->Result<usize,KeyboardControlError>{
            if principal==KeyboardControlPrincipal::Kernel && handle==1 && !self.keyboards.is_empty(){Ok(0)}else{Err(KeyboardControlError::Denied)}
        }
    }
    static STATION:spin::Mutex<Station>=spin::Mutex::new(Station{keyboards:Vec::new()});
    static REPORTS:spin::Mutex<Vec<(u8,[u8;6])>>=spin::Mutex::new(Vec::new());
    fn emit_report(keyboard:&Record){REPORTS.lock().push((keyboard.modifiers,keyboard.keys));}
    @RAW_REPORT@
    #[test] fn real_station_report_checks_owner_then_replaces_held_keys_and_modifiers(){
        *STATION.lock()=Station{keyboards:vec![Record{commands:vec![1],active:Some(1),modifiers:0,keys:[0;6],device_seq:0}]};
        REPORTS.lock().clear();
        assert_eq!(submit_report(KeyboardControlPrincipal::Vm(7),1,2,[4,5,0,0,0,0]),Err(KeyboardControlError::Denied));
        submit_report(KeyboardControlPrincipal::Kernel,1,2,[4,5,0,0,0,0]).unwrap();
        assert_eq!(*REPORTS.lock(),vec![(2,[4,5,0,0,0,0])]);
        {let station=STATION.lock();assert!(station.keyboards[0].commands.is_empty());assert!(station.keyboards[0].active.is_none());assert_eq!(station.keyboards[0].device_seq,1);}
        submit_report(KeyboardControlPrincipal::Kernel,1,1,[0;6]).unwrap();
        assert_eq!(REPORTS.lock().last(),Some(&(1,[0;6])));
        assert_eq!(submit_report(KeyboardControlPrincipal::Kernel,1,0,[1,0,0,0,0,0]),Err(KeyboardControlError::Invalid));
        assert_eq!(REPORTS.lock().len(),2);
        submit_report(KeyboardControlPrincipal::Kernel,1,0,[0;6]).unwrap();
        assert_eq!(REPORTS.lock().last(),Some(&(0,[0;6])));
    }
}
#[cfg(test)] mod tests {
    use super::*;
    use shell3::rdp::InputSession;
    fn setup()->InputSession{world(|w|*w=World::default());InputSession::new(AccountId(7)).unwrap()}
    fn packet(kind:u8,flags:u16,seq:u32,payload:&[u8])->Vec<u8>{
        let mut p=Vec::from(&b"THID\x01\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00"[..]);
        p[5]=kind;p[6..8].copy_from_slice(&flags.to_le_bytes());p[8..12].copy_from_slice(&seq.to_le_bytes());p.extend_from_slice(payload);p
    }
    fn tablet(x:i32,y:i32,buttons:u32,wheel:i16)->[u8;16]{let mut p=[0;16];p[..4].copy_from_slice(&x.to_le_bytes());p[4..8].copy_from_slice(&y.to_le_bytes());p[8..12].copy_from_slice(&buttons.to_le_bytes());p[12..14].copy_from_slice(&wheel.to_le_bytes());p}
    #[test] fn relative_motion_keeps_signed_deltas_at_display_edges(){
        let mut session=setup();
        session.input(&packet(3,1<<15,1,&tablet(320,-100,3,-2))).unwrap();
        assert_eq!(world(|w|*w.mouse_reports.last().unwrap()),(319,0,3,-2,Some((320,-100))));
        session.input(&packet(3,1<<15,2,&tablet(320,-100,0,0))).unwrap();
        assert_eq!(world(|w|*w.mouse_reports.last().unwrap()),(319,0,0,0,Some((320,-100))));
    }
    #[test] fn absolute_view_input_and_full_keyboard_reports_share_session(){
        let mut session=setup();session.input(&packet(3,0,1,&tablet(65535,65535,1,1))).unwrap();
        assert_eq!(world(|w|*w.mouse_reports.last().unwrap()),(319,199,1,1,None));
        session.input(&packet(2,0,1,&[2,0,4,5,0,0,0,0])).unwrap();
        assert_eq!(world(|w|(w.keyboards[0].2,w.keyboards[0].3)),(2,[4,5,0,0,0,0]));
        session.input(&packet(2,0,2,&[1,0,0,0,0,0,0,0])).unwrap();
        assert_eq!(world(|w|(w.keyboards[0].2,w.keyboards[0].3)),(1,[0;6]));
    }
    #[test] fn duplicate_records_do_not_repeat_wheel_or_reapply_old_keys(){
        let mut session=setup();let record=packet(3,1<<15,1,&tablet(1,2,0,1));
        session.input(&record).unwrap();let count=world(|w|w.mouse_reports.len());session.input(&record).unwrap();
        assert_eq!(world(|w|w.mouse_reports.len()),count);
    }
    #[test] fn control_feedback_and_release_use_existing_center_snap_contract(){
        let mut session=setup();world(|w|w.snapped=true);assert_eq!(session.control(),b"THIC\x01\x01\x01\x00");
        session.input(&packet(4,3,1,&[])).unwrap();assert_eq!(session.control(),b"THIC\x01\x00\x01\x00");
    }
    #[test] fn expired_session_cannot_submit_or_authenticate_devices(){
        let mut session=setup();let slot=world(|w|w.cursors[0].1);
        assert!(shell3::rdp::device_authority(usb2::hid::hut::HidDeviceKind::Mouse,slot).is_some());
        world(|w|w.sessions.clear());assert!(session.input(&packet(3,0,1,&tablet(0,0,0,0))).is_err());
        assert!(shell3::rdp::device_authority(usb2::hid::hut::HidDeviceKind::Mouse,slot).is_none());
    }
    #[test] fn disconnect_releases_all_owned_state_and_unrelated_sessions_remain(){
        let first=setup();let second=InputSession::new(AccountId(8)).unwrap();let second_id=second.id();drop(first);
        world(|w|{assert_eq!(w.keyboards.len(),1);assert_eq!(w.cursors.len(),1);assert_eq!(w.combos.len(),1);assert_eq!(w.sessions[0].session_id(),Some(second_id));});
        drop(second);world(|w|{assert!(w.sessions.is_empty());assert!(w.keyboards.is_empty());assert!(w.cursors.is_empty());assert!(w.combos.is_empty());});
    }
    #[test] fn failed_partial_allocation_cannot_leave_devices_or_a_grant(){
        world(|w|{*w=World::default();w.fail_cursor=true;});assert!(InputSession::new(AccountId(7)).is_err());
        world(|w|{assert!(w.keyboards.is_empty());assert!(w.sessions.is_empty());w.fail_cursor=false;w.fail_combo=true;});assert!(InputSession::new(AccountId(7)).is_err());
        world(|w|{assert!(w.keyboards.is_empty());assert!(w.cursors.is_empty());assert!(w.sessions.is_empty());});
    }
    #[test] fn malformed_records_and_rollover_keys_fail_before_input_effects(){
        let mut session=setup();let count=world(|w|w.mouse_reports.len());
        for bad in [packet(8,0,1,&[]),packet(3,0,1,&[]),packet(2,0,1,&[0;7]),packet(3,1<<15,1,&tablet(i32::MAX,0,0,0))]{assert!(session.input(&bad).is_err());}
        assert!(session.input(&packet(2,0,2,&[0,0,1,0,0,0,0,0])).is_err());
        assert_eq!(world(|w|w.mouse_reports.len()),count);
    }
}
