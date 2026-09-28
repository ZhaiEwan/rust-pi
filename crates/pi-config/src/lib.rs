//! 配置层：settings 文件、路径解析（含 XDG / home 展开）与环境变量。

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "pi-config");
    }
}
