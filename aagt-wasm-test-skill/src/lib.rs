#[no_mangle]
pub extern "C" fn allocate(size: u32) -> u32 {
    let mut buf = Vec::with_capacity(size as usize);
    let ptr = buf.as_mut_ptr() as *mut u8 as u32;
    std::mem::forget(buf);
    ptr
}

#[no_mangle]
pub extern "C" fn call(ptr: u32, len: u32) -> u64 {
    let input = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let input_str = String::from_utf8_lossy(input);

    let response = format!("WASM Skill received: {}", input_str);
    let response_bytes = response.into_bytes();
    let response_ptr = response_bytes.as_ptr() as u64;
    let response_len = response_bytes.len() as u64;

    std::mem::forget(response_bytes);

    (response_ptr << 32) | response_len
}
