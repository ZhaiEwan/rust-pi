//! 契约层：消息、内容块、usage、stop reason 与流式事件的类型定义。
//!
//! 本 crate 保持零内部依赖，所有对拍测试直接针对它的序列化结果。

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "pi-types");
    }
}
