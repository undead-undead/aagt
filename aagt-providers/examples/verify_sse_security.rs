use aagt_providers::utils::SseBuffer;

fn main() {
    println!("--- Verifying SseBuffer Security ---");

    // 1. Create limited buffer (10 bytes)
    let mut buffer = SseBuffer::with_capacity_limit(10);
    println!("> Created buffer with 10 bytes capacity");

    // 2. Push 5 bytes (OK)
    let data_ok = "12345".as_bytes();
    if let Ok(_) = buffer.extend_from_slice(data_ok) {
        println!("✅ Pushed 5 bytes: OK");
    } else {
        println!("❌ Failed to push 5 bytes (Unexpected)");
        std::process::exit(1);
    }

    // 3. Push 6 bytes (Total 11 > 10, Should Fail)
    let data_fail = "123456".as_bytes();
    match buffer.extend_from_slice(data_fail) {
        Err(e) => {
            println!("✅ Overflow rejected: {}", e);
        }
        Ok(_) => {
            println!("❌ Overflow ALLOWED (Security Vulnerability!)");
            std::process::exit(1);
        }
    }

    println!("--- Verification Success ---");
}
