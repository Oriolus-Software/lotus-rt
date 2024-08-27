use std::time::Duration;

use lotus_rt::wait;

fn main() {
    let start = std::time::Instant::now();
    lotus_rt::spawn(async move {
        lotus_rt::spawn(async move {
            println!("Hello, nested world!");
        });

        println!("Hello, world!");
    });

    lotus_rt::spawn(async move {
        println!("Loading, please wait...");
        let addition = lotus_rt::spawn(add(3, 5));
        println!("3 + 5 = {}", addition.into_future().await.unwrap());
    });

    lotus_rt::spawn(async move {
        wait::ticks(3).await;
        println!("After technically 4 ticks");
    });

    lotus_rt::spawn(async move {
        wait::seconds(1.0).await;
        println!("After 1 second");
    });

    let (tx, rx) = lotus_rt::sync::oneshot::channel();

    lotus_rt::spawn(async move {
        println!("Received {}", rx.await.unwrap());
    });

    lotus_rt::spawn(async move {
        println!("Sending 42");
        tx.send(42).unwrap();
    });

    lotus_rt::spawn(async move {
        let (a, b) = lotus_rt::join!(generate_value(42), generate_value(7));
        println!("{} + {} = {}", a, b, a + b);
    });

    lotus_rt::spawn(async move {
        wait::ticks(5).await;

        println!("After technically 6 ticks");
    });

    for i in 0..5 {
        println!("Tick {i}");
        lotus_rt::tick();
    }

    std::thread::sleep(Duration::from_secs(1));

    lotus_rt::tick();

    let elapsed = start.elapsed();
    println!("Elapsed: {:?}", elapsed);
}

async fn generate_value<T>(val: T) -> T {
    val
}

async fn add(a: i32, b: i32) -> i32 {
    wait::next_tick().await;
    a + b
}
