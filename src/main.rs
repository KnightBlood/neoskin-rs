//! neoskin-rs - JSON:API Server Entry Point

use suprnova::Application;

use neoskin_rs::{bootstrap, config, migrations, routes};

/// 主线程默认栈在 Windows 上仅 1 MiB，而 debug 构建下启动期（迁移、
/// 路由构建与 bootstrap）的帧远大于此，会触发 stack overflow。把入口
/// 放到一个显式指定栈大小的线程里执行。
fn main() {
    let worker = std::thread::Builder::new()
        .name("neoskin-main".to_string())
        .stack_size(16 * 1024 * 1024)
        .spawn(entry)
        .expect("failed to spawn neoskin-main thread");
    if let Err(payload) = worker.join() {
        std::panic::resume_unwind(payload);
    }
}

#[suprnova::main]
async fn entry() {
    Application::new()
        .config(config::register_all)
        .bootstrap(bootstrap::register)
        .routes(routes::register)
        .migrations::<migrations::Migrator>()
        .run()
        .await;
}
