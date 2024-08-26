fn main() {
    lotus_rt::spawn(async move {
        lotus_rt::spawn(async move {
            println!("Hello, nested world!");
        });

        println!("Hello, world!");

        lotus_rt::spawn(async move {
            println!("3 + 5 = {}", add(3, 5).await);
        });
    });

    lotus_rt::tick();
    lotus_rt::tick();
}

async fn add(a: i32, b: i32) -> i32 {
    lotus_rt::next_tick().await;
    a + b
}
