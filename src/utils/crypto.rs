pub fn generate_recovery_code(key: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    // 计算密钥哈希
    let mut hasher = Sha256::new();
    hasher.update(key);
    let hash = hasher.finalize();

    // 转换为单词列表（便于记忆）
    let wordlist = vec![
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
        "juliet", "kilo", "lima", "mike", "november", "oscar", "papa", "quebec", "romeo", "sierra",
        "tango", "uniform", "victor", "whiskey", "xray", "yankee", "zulu", "zero", "one", "two",
        "three", "four", "five", "six", "seven", "eight", "nine",
    ];

    let mut words = Vec::new();
    for chunk in hash.chunks(2) {
        let index = ((chunk[0] as usize) << 8 | chunk[1] as usize) % wordlist.len();
        words.push(wordlist[index]);
    }

    // 取前8个单词
    words[..8].join("-")
}
