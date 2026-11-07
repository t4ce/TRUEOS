#[cfg(test)]
mod tests {
    use super::*;
    use crate::vnetfs::http::{self, BodyRead, Header, Method, Timeouts};
    use std::sync::atomic::Ordering;
    use std::task::{Context, Poll};
    fn reset() {
        let mut registry=REGISTRY.lock();
        registry.entries.clear();registry.pending.clear();registry.next=1;registry.closed_runs=[None;256];
        crate::hv::GENERATION.store(7,Ordering::SeqCst);
        crate::vcabi::DISCARDS.store(0,Ordering::SeqCst);
        *crate::vcabi::MOCK.lock()=None;
        *crate::vmcall::RESULT.lock()=(1,0,0);
    }
    fn request()->RequestOwned {RequestOwned{method:Method::Get,url:"https://example.test/a?q=1".into(),headers:vec![Header{name:"x-repeat".into(),value:vec![128,255]},Header{name:"x-repeat".into(),value:b"second".to_vec()}],timeouts:Timeouts::default()}}
    fn frame()->Vec<u8> {wire::encode_request(&request()).unwrap()}
    fn start(owner:Owner)->(u32,ResponseSink) {
        let id=start_host(owner,&frame());assert!(id>0,"{id}");
        let pending=REGISTRY.lock().pending.pop_front().unwrap();assert_eq!(pending.request,request());
        (id as u32,pending.sink)
    }
    fn head()->ResponseHead {ResponseHead{status:206,url:request().url,headers:request().headers}}
    fn publish(sink:&ResponseSink) {sink.publish_head(head()).unwrap();}
    struct CountWake(std::sync::atomic::AtomicUsize);
    impl std::task::Wake for CountWake {fn wake(self:Arc<Self>) {self.0.fetch_add(1,Ordering::SeqCst);} fn wake_by_ref(self:&Arc<Self>){self.0.fetch_add(1,Ordering::SeqCst);}}
    fn waker()->(Arc<CountWake>,std::task::Waker) {let count=Arc::new(CountWake(std::sync::atomic::AtomicUsize::new(0)));(count.clone(),std::task::Waker::from(count))}
    fn ready<F:Future>(future:F)->F::Output {let(_,waker)=waker();let mut cx=Context::from_waker(&waker);let mut future=std::pin::pin!(future);match future.as_mut().poll(&mut cx){Poll::Ready(value)=>value,Poll::Pending=>panic!("unexpected pending")}}
    #[test] fn codec_request_head_roundtrip_raw_duplicates_and_timeouts() {
        reset();let mut req=request();req.method=Method::Head;req.timeouts=Timeouts{connect_ms:17,tls_ms:u32::MAX,read_idle_ms:0};
        assert_eq!(wire::decode_request(&wire::encode_request(&req).unwrap()).unwrap(),req);
        let h=head();assert_eq!(wire::decode_head(&wire::encode_head(&h).unwrap()).unwrap(),h);
        assert_eq!(h.values("X-Repeat").collect::<Vec<_>>(),[&[128,255][..],&b"second"[..]]);
    }
    #[test] fn codec_all_truncations_trailing_reserved_method_and_count_rejected() {
        let f=frame();for len in 0..f.len(){assert!(wire::decode_request(&f[..len]).is_err(),"{len}");}
        let mut bad=f.clone();bad.push(0);assert!(wire::decode_request(&bad).is_err());
        for index in [4,5] {let mut bad=f.clone();bad[index]=255;assert!(wire::decode_request(&bad).is_err());}
        let mut bad=f;bad[6..8].copy_from_slice(&129u16.to_le_bytes());assert!(wire::decode_request(&bad).is_err());
        let h=wire::encode_head(&head()).unwrap();for len in 0..h.len(){assert!(wire::decode_head(&h[..len]).is_err());}
    }
    #[test] fn codec_bounds_url_and_header_injection_before_allocating_payload() {
        for url in ["file:///a","https:///a","https://user@example.test/","http://example.test/#secret","http://example.test/\r\n"] {let mut r=request();r.url=url.into();assert!(wire::encode_request(&r).is_err(),"{url}");}
        for (name,value) in [("bad name",b"x".to_vec()),("ok",b"x\r\ny".to_vec()),("ok",vec![0]),("ok",vec![127])] {let mut r=request();r.headers=vec![Header{name:name.into(),value}];assert!(wire::encode_request(&r).is_err());}
        let mut r=request();r.headers.clear();r.url="https://e/".into();let fixed=wire::encode_request(&r).unwrap().len();r.url.push_str(&"a".repeat(MAX_FRAME_BYTES-fixed));assert_eq!(wire::encode_request(&r).unwrap().len(),MAX_FRAME_BYTES);r.url.push('a');assert_eq!(wire::encode_request(&r),Err(FS_ERR_TOO_LARGE));
    }
    #[test] fn pending_is_distinct_from_eof_and_head_stays_immutable() {
        reset();let(id,sink)=start(Owner::Kernel);let mut out=[0;32];
        assert_eq!(head_host(Owner::Kernel,id,&mut[]),NET_PENDING as isize);assert_eq!(read_host(Owner::Kernel,id,&mut out),NET_PENDING as isize);
        publish(&sink);let n=head_host(Owner::Kernel,id,&mut[]);assert!(n>0);let mut bytes=vec![0;n as usize];assert_eq!(head_host(Owner::Kernel,id,&mut bytes),n);assert_eq!(wire::decode_head(&bytes).unwrap(),head());
        assert_eq!(sink.publish_head(head()),Err(FS_ERR_BAD_PARAM));sink.finish(Ok(()));assert_eq!(read_host(Owner::Kernel,id,&mut out),0);assert_eq!(head_host(Owner::Kernel,id,&mut[]),n);
    }
    #[test] fn prefix_drains_before_stable_terminal_error() {
        reset();let(id,sink)=start(Owner::Kernel);publish(&sink);ready(sink.push_body(b"abcdef")).unwrap();sink.finish(Err(NET_ERR_RESPONSE_FRAMING));sink.finish(Ok(()));
        let mut out=[0;4];assert_eq!(read_host(Owner::Kernel,id,&mut out),4);assert_eq!(&out,b"abcd");assert_eq!(read_host(Owner::Kernel,id,&mut out),2);assert_eq!(&out[..2],b"ef");
        for _ in 0..2 {assert_eq!(read_host(Owner::Kernel,id,&mut out),NET_ERR_RESPONSE_FRAMING as isize);}
        assert!(head_host(Owner::Kernel,id,&mut[])>0);
    }
    #[test] fn body_window_backpressure_releases_credit_without_losing_bytes() {
        reset();let(id,sink)=start(Owner::Kernel);publish(&sink);ready(sink.push_body(&vec![3;BODY_WINDOW])).unwrap();
        let(count,waker)=waker();let mut cx=Context::from_waker(&waker);let mut blocked=std::pin::pin!(sink.push_body(b"next"));assert!(blocked.as_mut().poll(&mut cx).is_pending());
        let mut out=[0;4];assert_eq!(read_host(Owner::Kernel,id,&mut out),4);assert_eq!(out,[3;4]);assert!(count.0.load(Ordering::SeqCst)>0);assert_eq!(blocked.as_mut().poll(&mut cx),Poll::Ready(Ok(())));assert_eq!(sink.shared.state.lock().body.len(),BODY_WINDOW);
    }
    #[test] fn discard_wakes_blocked_credit_and_cancellation_and_prevents_late_publication() {
        reset();let(id,sink)=start(Owner::Kernel);publish(&sink);ready(sink.push_body(&vec![4;BODY_WINDOW])).unwrap();
        let(count,waker)=waker();let mut cx=Context::from_waker(&waker);let mut push=std::pin::pin!(sink.push_body(b"blocked"));let mut cancelled=std::pin::pin!(sink.cancelled());assert!(push.as_mut().poll(&mut cx).is_pending());assert!(cancelled.as_mut().poll(&mut cx).is_pending());
        assert_eq!(discard_host(Owner::Kernel,id),0);assert!(count.0.load(Ordering::SeqCst)>0);assert_eq!(push.as_mut().poll(&mut cx),Poll::Ready(Err(FS_ERR_NOT_FOUND)));assert!(cancelled.as_mut().poll(&mut cx).is_ready());
        assert_eq!(sink.publish_head(head()),Err(FS_ERR_NOT_FOUND));sink.finish(Ok(()));assert!(sink.shared.state.lock().body.is_empty());assert!(sink.shared.state.lock().head.is_none());assert_eq!(head_host(Owner::Kernel,id,&mut[]),FS_ERR_NOT_FOUND as isize);
        assert!(REGISTRY.lock().entries.contains_key(&id));producer_retired(&sink);assert!(!REGISTRY.lock().entries.contains_key(&id));sink.finish(Err(FS_ERR_IO));assert!(!REGISTRY.lock().entries.contains_key(&id));
    }
    #[test] fn owner_and_generation_isolation_for_every_operation() {
        reset();let owner=Owner::Vm{id:0,generation:7};let(id,sink)=start(owner);publish(&sink);ready(sink.push_body(b"safe")).unwrap();let mut out=[0;4];
        for foreign in [Owner::Kernel,Owner::Vm{id:1,generation:7},Owner::Vm{id:0,generation:6}] {assert_eq!(head_host(foreign,id,&mut[]),FS_ERR_NOT_FOUND as isize);assert_eq!(read_host(foreign,id,&mut out),FS_ERR_NOT_FOUND as isize);assert_eq!(discard_host(foreign,id),FS_ERR_NOT_FOUND);}
        assert_eq!(read_host(owner,id,&mut out),4);assert_eq!(&out,b"safe");crate::hv::GENERATION.store(8,Ordering::SeqCst);assert_eq!(head_host(owner,id,&mut[]),FS_ERR_NOT_FOUND as isize);assert!(sink.is_cancelled());
        assert_eq!(discard_for_vm(0,7),1);producer_retired(&sink);assert!(REGISTRY.lock().entries.is_empty());
    }
    #[test] fn teardown_reclaims_completed_head_and_body_for_exact_run_only() {
        reset();let owner=Owner::Vm{id:0,generation:7};let(id,sink)=start(owner);publish(&sink);ready(sink.push_body(b"ready body")).unwrap();sink.finish(Ok(()));producer_retired(&sink);
        let(other,other_sink)=start(Owner::Vm{id:1,generation:7});publish(&other_sink);assert_eq!(discard_for_vm(0,6),0);assert_eq!(discard_for_vm(0,7),1);assert!(!REGISTRY.lock().entries.contains_key(&id));assert!(REGISTRY.lock().entries.contains_key(&other));assert!(sink.shared.state.lock().head.is_none());assert!(sink.shared.state.lock().body.is_empty());
    }
    #[test] fn retiring_operations_still_count_against_per_owner_admission() {
        reset();let mut sinks=Vec::new();for _ in 0..MAX_OPERATIONS_PER_OWNER {let(id,sink)=start(Owner::Kernel);assert_eq!(discard_host(Owner::Kernel,id),0);sinks.push(sink);}
        assert_eq!(start_host(Owner::Kernel,&frame()),FS_ERR_NO_SPACE);producer_retired(&sinks.pop().unwrap());assert!(start_host(Owner::Kernel,&frame())>0);
    }
    #[test] fn global_admission_and_id_exhaustion_do_not_reuse_handles() {
        reset();for owner in [Owner::Kernel,Owner::Vm{id:0,generation:7}] {for _ in 0..MAX_OPERATIONS_PER_OWNER {let _=start(owner);}}
        assert_eq!(start_host(Owner::Vm{id:1,generation:7},&frame()),FS_ERR_NO_SPACE);reset();REGISTRY.lock().next=i32::MAX as u32;let(id,_)=start(Owner::Kernel);assert_eq!(id,i32::MAX as u32);assert_eq!(start_host(Owner::Kernel,&frame()),FS_ERR_NO_SPACE);
    }
    #[test] fn invalid_caps_and_short_head_do_not_consume_data() {
        reset();let(id,sink)=start(Owner::Kernel);publish(&sink);ready(sink.push_body(b"bytes")).unwrap();assert_eq!(read_host(Owner::Kernel,id,&mut[]),FS_ERR_BAD_PARAM as isize);assert_eq!(read_host(Owner::Kernel,id,&mut vec![0;MAX_BODY_READ_BYTES+1]),FS_ERR_BAD_PARAM as isize);assert_eq!(head_host(Owner::Kernel,id,&mut[0]),FS_ERR_TOO_LARGE as isize);let mut out=[0;5];assert_eq!(read_host(Owner::Kernel,id,&mut out),5);assert_eq!(&out,b"bytes");
    }
    #[test] fn vm_arguments_are_validated_before_cast_or_payload_consumption() {
        use crate::vmcall::*;assert_eq!(decode_vm_command(OP_BP_HTTP_REQUEST_START_V1,0,0,MAX_FRAME_BYTES),Ok(VmCommand::Start));
        for (op,a,b,n) in [(OP_BP_HTTP_REQUEST_START_V1,1,0,24),(OP_BP_HTTP_REQUEST_START_V1,0,1,24),(OP_BP_HTTP_REQUEST_START_V1,0,0,0),(OP_BP_HTTP_REQUEST_START_V1,0,0,MAX_FRAME_BYTES+1),(OP_BP_HTTP_REQUEST_READ_V1,u64::MAX,1,0),(OP_BP_HTTP_REQUEST_READ_V1,1,u64::MAX,0),(OP_BP_HTTP_REQUEST_READ_V1,1,0,0),(OP_BP_HTTP_REQUEST_HEAD_V1,0,0,0),(OP_BP_HTTP_REQUEST_HEAD_V1,1,1,1),(OP_BP_HTTP_REQUEST_DISCARD_V1,1,1,0),(OP_BP_HTTP_REQUEST_DISCARD_V1,1,0,1),(0,1,0,0)] {assert_eq!(decode_vm_command(op,a,b,n),Err(FS_ERR_BAD_PARAM));}
        assert_eq!(decode_vm_command(OP_BP_HTTP_REQUEST_HEAD_V1,1,0,0),Ok(VmCommand::Head{id:1,cap:0}));assert_eq!(decode_vm_command(OP_BP_HTTP_REQUEST_READ_V1,1,MAX_BODY_READ_BYTES as u64,0),Ok(VmCommand::Read{id:1,cap:MAX_BODY_READ_BYTES}));
    }
    #[test] fn sdk_raii_pending_prefix_error_and_exact_single_discard() {
        reset();{
            let mut req=http::Request::start(&request()).unwrap();let pending=REGISTRY.lock().pending.pop_front().unwrap();assert_eq!(req.try_head(),Ok(None));publish(&pending.sink);assert_eq!(req.try_head(),Ok(Some(head())));ready(pending.sink.push_body(b"body")).unwrap();pending.sink.finish(Err(NET_ERR_TIMEOUT_BODY));let mut out=[0;8];assert_eq!(req.try_read(&mut out),Ok(BodyRead::Data(4)));assert_eq!(&out[..4],b"body");assert_eq!(req.try_read(&mut out),Err(NET_ERR_TIMEOUT_BODY));req.discard().unwrap();
        } assert_eq!(crate::vcabi::DISCARDS.load(Ordering::SeqCst),1);
        {let _req=http::Request::start(&request()).unwrap();}assert_eq!(crate::vcabi::DISCARDS.load(Ordering::SeqCst),2);
    }
    #[test] fn sdk_rejects_bad_start_and_empty_read_without_losing_ownership() {
        reset();let mut bad=request();bad.url="file:///oops".into();assert!(http::Request::start(&bad).is_err());assert!(REGISTRY.lock().entries.is_empty());let mut req=http::Request::start(&request()).unwrap();assert_eq!(req.try_read(&mut[]),Err(FS_ERR_BAD_PARAM));drop(req);assert_eq!(crate::vcabi::DISCARDS.load(Ordering::SeqCst),1);
    }
    #[test] fn teardown_closes_admission_before_hv_generation_changes() {
        reset();let owner=Owner::Vm{id:0,generation:7};assert_eq!(discard_for_vm(0,7),0);
        assert_eq!(crate::hv::vm_run_generation(0),Some(7));assert_eq!(start_host(owner,&frame()),FS_ERR_NOT_FOUND);
        assert!(REGISTRY.lock().entries.is_empty());assert!(start_host(Owner::Vm{id:1,generation:7},&frame())>0);
        crate::hv::GENERATION.store(8,Ordering::SeqCst);assert!(start_host(Owner::Vm{id:0,generation:8},&frame())>0);
    }
    #[test] fn stale_pending_producer_self_retires_without_body_or_handle() {
        reset();let owner=Owner::Vm{id:0,generation:7};let(id,sink)=start(owner);publish(&sink);ready(sink.push_body(b"old run")).unwrap();
        crate::hv::GENERATION.store(8,Ordering::SeqCst);producer_retired(&sink);
        assert!(!REGISTRY.lock().entries.contains_key(&id));assert!(sink.shared.state.lock().head.is_none());assert!(sink.shared.state.lock().body.is_empty());
    }

    #[test] fn sdk_invalid_start_ids_never_create_drop_ownership() {
        for value in [0,-3,-10,-116] {reset();*crate::vcabi::MOCK.lock()=Some(crate::vcabi::Mock{start:value,..Default::default()});assert_eq!(http::Request::start(&request()).unwrap_err(),if value==0{-4}else{value});assert_eq!(crate::vcabi::DISCARDS.load(Ordering::SeqCst),0);}
    }
    #[test] fn sdk_head_rejects_negative_zero_oversize_short_and_extra_counts() {
        for query in [-2,0,MAX_FRAME_BYTES as isize+1] {reset();*crate::vcabi::MOCK.lock()=Some(crate::vcabi::Mock{query,..Default::default()});let mut req=http::Request::start(&request()).unwrap();assert!(req.try_head().is_err());drop(req);assert_eq!(crate::vcabi::DISCARDS.load(Ordering::SeqCst),1);}
        let encoded=wire::encode_head(&head()).unwrap();for count in [-2,encoded.len() as isize-1,encoded.len() as isize+1] {reset();*crate::vcabi::MOCK.lock()=Some(crate::vcabi::Mock{query:encoded.len() as isize,head_read:count,head:encoded.clone(),..Default::default()});let mut req=http::Request::start(&request()).unwrap();assert!(req.try_head().is_err());drop(req);assert_eq!(crate::vcabi::DISCARDS.load(Ordering::SeqCst),1);}
    }
    #[test] fn sdk_read_validates_return_count_clamps_capacity_and_discards_once_on_error() {
        for (read,expected) in [(0,Ok(BodyRead::Eof)),(-116,Ok(BodyRead::Pending)),(-115,Err(-115)),(32768,Ok(BodyRead::Data(32768))),(32769,Err(-2))] {
            reset();*crate::vcabi::MOCK.lock()=Some(crate::vcabi::Mock{read,..Default::default()});let mut req=http::Request::start(&request()).unwrap();assert_eq!(req.try_read(&mut vec![0;MAX_BODY_READ_BYTES+8]),expected);assert_eq!(crate::vcabi::MOCK.lock().as_ref().unwrap().last_cap,MAX_BODY_READ_BYTES);drop(req);assert_eq!(crate::vcabi::DISCARDS.load(Ordering::SeqCst),1);
        }
    }
    #[test] fn sdk_explicit_discard_error_still_consumes_ownership_exactly_once() {
        reset();*crate::vcabi::MOCK.lock()=Some(crate::vcabi::Mock{discard:-8,..Default::default()});let req=http::Request::start(&request()).unwrap();assert_eq!(req.discard(),Err(-8));assert_eq!(crate::vcabi::DISCARDS.load(Ordering::SeqCst),1);
    }
    #[test] fn guest_response_signed_errors_and_reported_lengths_are_checked_exactly() {
        use crate::vmcall::*;
        for code in [-116isize,-115,-8,0] {*RESULT.lock()=(STATUS_OK,(code as i64) as u64,0);assert_eq!(guest_call(OP_BP_HTTP_REQUEST_READ_V1,1,4,&[],&mut[0;4]),code);}
        *RESULT.lock()=(STATUS_OK,4,4);assert_eq!(guest_call(OP_BP_HTTP_REQUEST_READ_V1,1,4,&[],&mut[0;4]),4);
        for (value,reported) in [(4,3),(4,5),(5,5),(0,1),((-116i64) as u64,1)] {*RESULT.lock()=(STATUS_OK,value,reported);assert_eq!(guest_call(OP_BP_HTTP_REQUEST_READ_V1,1,4,&[],&mut[0;4]),FS_ERR_IO as isize);}
        *RESULT.lock()=(STATUS_OK,100,0);assert_eq!(guest_call(OP_BP_HTTP_REQUEST_HEAD_V1,1,0,&[],&mut[]),100);
        *RESULT.lock()=(STATUS_OK,100,1);assert_eq!(guest_call(OP_BP_HTTP_REQUEST_HEAD_V1,1,0,&[],&mut[]),FS_ERR_IO as isize);
        *RESULT.lock()=(1,4,4);assert_eq!(guest_call(OP_BP_HTTP_REQUEST_READ_V1,1,4,&[],&mut[0;4]),FS_ERR_BAD_PARAM as isize);
    }
    #[test] fn cabi_rejects_null_or_oversize_before_constructing_pointer_slices() {
        reset();unsafe {
            assert_eq!(trueos_cabi_net_http_request_start_v1(core::ptr::null(),24),FS_ERR_BAD_PARAM);
            assert_eq!(trueos_cabi_net_http_request_start_v1(1usize as *const u8,MAX_FRAME_BYTES+1),FS_ERR_BAD_PARAM);
            assert_eq!(trueos_cabi_net_http_request_head_v1(1,core::ptr::null_mut(),1),FS_ERR_BAD_PARAM as isize);
            assert_eq!(trueos_cabi_net_http_request_read_v1(1,1usize as *mut u8,usize::MAX),FS_ERR_BAD_PARAM as isize);
            assert_eq!(trueos_cabi_net_http_request_read_v1(1,core::ptr::null_mut(),0),FS_ERR_BAD_PARAM as isize);
        }
    }
    #[test] fn codec_corrupt_utf8_lengths_names_status_and_trailing_head_rejected() {
        let f=frame();let mut bad_length=f.clone();bad_length[8..12].fill(255);assert!(wire::decode_request(&bad_length).is_err());
        let mut bad=f.clone();bad[24]=255;assert!(wire::decode_request(&bad).is_err());
        let mut bad=f;let url_len=u32::from_le_bytes(bad[8..12].try_into().unwrap()) as usize;bad[24+url_len+6]=b' ';assert!(wire::decode_request(&bad).is_err());
        let h=wire::encode_head(&head()).unwrap();for status in [0u16,99,1000,u16::MAX] {let mut bad=h.clone();bad[4..6].copy_from_slice(&status.to_le_bytes());assert!(wire::decode_head(&bad).is_err());}
        let mut bad=h;bad.push(0);assert!(wire::decode_head(&bad).is_err());
    }

}
