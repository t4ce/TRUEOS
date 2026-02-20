extern crate alloc;

use alloc::vec::Vec;

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_fs_read_file(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    fn trueos_cabi_fs_write_begin(
        path_ptr: *const u8,
        path_len: usize,
        total_len: u64,
        out_handle: *mut u32,
    ) -> i32;
    fn trueos_cabi_fs_write_chunk(handle: u32, data_ptr: *const u8, data_len: usize) -> i32;
    fn trueos_cabi_fs_write_finish(handle: u32) -> i32;
    fn trueos_cabi_fs_write_abort(handle: u32) -> i32;
}

#[inline]
fn push_i32_dec(out: &mut Vec<u8>, v: i32) {
    if v == 0 {
        out.push(b'0');
        return;
    }
    let neg = v < 0;
    let mut n = if neg { -(v as i64) as u64 } else { v as u64 };
    let mut buf = [0u8; 16];
    let mut i = buf.len();
    while n != 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if neg {
        out.push(b'-');
    }
    out.extend_from_slice(&buf[i..]);
}

#[inline]
fn trace_str(s: &str) {
    super::trace_str(s);
}

#[inline]
fn trace_bytes(bytes: &[u8]) {
    super::trace_bytes(bytes);
}

#[inline]
fn trace_nl() {
    super::trace_nl();
}

#[inline]
fn trace_usize_dec(v: usize) {
    super::trace_usize_dec(v);
}

fn read_file_sync(path: &[u8]) -> Result<Vec<u8>, i32> {
    let len =
        unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(len as i32);
    }
    let len = len as usize;
    let mut out = Vec::new();
    out.resize(len, 0);
    let got =
        unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), out.as_mut_ptr(), out.len()) };
    if got < 0 {
        return Err(got as i32);
    }
    out.truncate(got as usize);
    Ok(out)
}

fn write_file_sync(path: &[u8], data: &[u8]) -> Result<(), i32> {
    let mut handle: u32 = 0;
    let rc = unsafe {
        trueos_cabi_fs_write_begin(
            path.as_ptr(),
            path.len(),
            data.len() as u64,
            &mut handle as *mut u32,
        )
    };
    if rc != 0 {
        return Err(rc);
    }
    let rc = unsafe { trueos_cabi_fs_write_chunk(handle, data.as_ptr(), data.len()) };
    if rc != 0 {
        let _ = unsafe { trueos_cabi_fs_write_abort(handle) };
        return Err(rc);
    }
    let rc = unsafe { trueos_cabi_fs_write_finish(handle) };
    if rc != 0 {
        let _ = unsafe { trueos_cabi_fs_write_abort(handle) };
        return Err(rc);
    }
    Ok(())
}

pub(super) fn compiled_cache_path_for_source(source_cache_path: &[u8]) -> Vec<u8> {
    let mut out = source_cache_path.to_vec();
    if out.ends_with(b".mjs") {
        out.truncate(out.len().saturating_sub(4));
    }
    out.extend_from_slice(b".qjsc");
    out
}

pub(super) unsafe fn try_load_compiled_module(
    ctx: *mut qjs::JSContext,
    compiled_path: &[u8],
) -> Result<*mut qjs::JSModuleDef, i32> {
    let buf = read_file_sync(compiled_path)?;
    if buf.is_empty() {
        return Err(super::FS_ERR_NOT_FOUND);
    }

    let v = qjs::JS_ReadObject(ctx, buf.as_ptr(), buf.len(), qjs::JS_READ_OBJ_BYTECODE);
    if v.is_exception() {
        return Err(super::FS_ERR_IO);
    }
    if v.tag != qjs::JS_TAG_MODULE {
        qjs::js_free_value(ctx, v);
        return Err(super::FS_ERR_IO);
    }
    let m = v.u.ptr as *mut qjs::JSModuleDef;
    qjs::js_free_value(ctx, v);
    Ok(m)
}

pub(super) unsafe fn persist_compiled_module(
    ctx: *mut qjs::JSContext,
    compiled_path: &[u8],
    module_value: qjs::JSValueConst,
) {
    let mut blob_len = 0usize;
    let blob_ptr = qjs::JS_WriteObject(
        ctx,
        &mut blob_len as *mut usize,
        module_value,
        qjs::JS_WRITE_OBJ_BYTECODE,
    );
    if blob_ptr.is_null() || blob_len == 0 {
        return;
    }

    let blob = core::slice::from_raw_parts(blob_ptr, blob_len);
    match write_file_sync(compiled_path, blob) {
        Ok(()) => {
            trace_str("qjs: compiled cache write ok path=");
            trace_bytes(compiled_path);
            trace_str(" len=");
            trace_usize_dec(blob_len);
            trace_nl();
        }
        Err(rc) => {
            trace_str("qjs: compiled cache write fail rc=");
            let mut tmp = Vec::new();
            push_i32_dec(&mut tmp, rc);
            trace_bytes(&tmp);
            trace_str(" path=");
            trace_bytes(compiled_path);
            trace_nl();
        }
    }

    qjs::js_free(ctx, blob_ptr as *mut core::ffi::c_void);
}
