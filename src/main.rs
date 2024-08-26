use std::time::Duration;

use lotus_rt::wait;

fn main() {
    lotus_rt::spawn(async move {
        lotus_rt::spawn(async move {
            println!("Hello, nested world!");
        });

        println!("Hello, world!");
    });

    lotus_rt::spawn(async move {
        println!("Loading, please wait...");
        println!("3 + 5 = {}", add(3, 5).await);
    });

    lotus_rt::spawn(async move {
        wait::ticks(3).await;
        println!("After techically 4 ticks");
    });

    lotus_rt::spawn(async move {
        wait::seconds(1.0).await;
        println!("After 1 second");
    });

    lotus_rt::tick();
    lotus_rt::tick();
    lotus_rt::tick();
    lotus_rt::tick();

    std::thread::sleep(Duration::from_secs(1));

    lotus_rt::tick();
}

async fn add(a: i32, b: i32) -> i32 {
    wait::next_tick().await;
    a + b
}
