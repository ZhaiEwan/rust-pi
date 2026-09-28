//! Agent 循环：请求 → 流式响应 → 工具执行 → 下一轮，以及上下文装配与系统提示。

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "pi-agent");
    }
}
