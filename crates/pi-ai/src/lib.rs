//! 供应商层：provider 抽象、协议族（Anthropic / OpenAI 等）、SSE 解析、
//! 模型目录与认证。

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "pi-ai");
    }
}
