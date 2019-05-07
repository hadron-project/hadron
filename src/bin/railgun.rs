use env_logger;
use railgun::App;

fn main() {
    env_logger::init();

    // Create and run the app. This call blocks.
    App::new().run()
}
