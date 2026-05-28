// Pages — Rust ports of capsules/hey-social/client/src/pages/*.jsx.
//
// Each one is a Leptos component routed from lib.rs. Auth-gated pages
// (everything other than Landing/SignIn/SignUp/Onboarding) get the
// global SignInGate redirect for free.

pub mod chat;
pub mod clips;
pub mod home;
pub mod landing;
pub mod misc;
pub mod onboarding;
pub mod post_detail;
pub mod posts;
pub mod profile;
pub mod signin;
pub mod signup;
pub mod video_player;

pub use chat::Chat;
pub use clips::Clips;
pub use home::Home;
pub use landing::Landing;
pub use misc::NotFound;
pub use onboarding::Onboarding;
pub use post_detail::PostDetail;
pub use posts::Posts;
pub use profile::Profile;
pub use signin::SignIn;
pub use signup::SignUp;
pub use video_player::VideoPlayer;
