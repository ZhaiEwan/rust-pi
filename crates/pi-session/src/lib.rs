//! 会话持久化：JSONL 条目的读写、树形分支（parent id）、fork / clone / 压缩。

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "pi-session");
    }
}
