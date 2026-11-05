//! 运行时冒烟：连接 `.env` 的 SQLite，读取各模型（Phase 1 校验用）。

use neoskin_rs::models::{
    closet_item, communication, job, language_line, notification, oauth, option, player, report,
    scope, texture, user,
};
use neoskin_rs::{bootstrap, config};
use sea_orm::EntityTrait;
use suprnova::{DB, Model};

#[suprnova::main]
async fn main() {
    config::register_all();
    bootstrap::register().await;

    let connection = DB::connection().expect("DB not initialized");
    let conn = connection.inner();

    println!("users={}", user::User::all().await.expect("users").len());
    println!(
        "players={}",
        player::Player::all().await.expect("players").len()
    );
    println!(
        "textures={}",
        texture::Entity::find()
            .all(conn)
            .await
            .expect("textures")
            .len()
    );
    println!(
        "options={}",
        option::OptionItem::all().await.expect("options").len()
    );
    println!(
        "reports={}",
        report::Report::all().await.expect("reports").len()
    );
    println!("jobs={}", job::Job::all().await.expect("jobs").len());
    println!(
        "language_lines={}",
        language_line::LanguageLine::all()
            .await
            .expect("language_lines")
            .len()
    );
    println!(
        "scopes={}",
        scope::Scope::all().await.expect("scopes").len()
    );
    println!(
        "communications={}",
        communication::Communication::all()
            .await
            .expect("communications")
            .len()
    );
    println!(
        "oauth_clients={}",
        oauth::OAuthClient::all()
            .await
            .expect("oauth_clients")
            .len()
    );
    println!(
        "notifications={}",
        notification::Entity::find()
            .all(conn)
            .await
            .expect("notifications")
            .len()
    );
    println!(
        "closet_items={}",
        closet_item::Entity::find()
            .all(conn)
            .await
            .expect("user_closet")
            .len()
    );
}
