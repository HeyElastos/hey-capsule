// SignInGate — runs at App level. Watches the current URL + session and
// redirects any unauthenticated visit to a protected route back to /signin.
// Public routes ("/", "/signin", "/signup", "/onboarding") are exempt.

use leptos::prelude::*;
use leptos_router::hooks::{use_location, use_navigate};
use leptos_router::NavigateOptions;

use crate::session;

#[component]
pub fn SignInGate() -> impl IntoView {
    let location = use_location();
    let navigate = use_navigate();
    Effect::new(move |_| {
        let path = location.pathname.get();
        let public = matches!(
            path.as_str(),
            "/" | "/signin" | "/signup" | "/onboarding"
        );
        if !public && session::current().is_none() {
            navigate("/signin", NavigateOptions::default());
        }
    });
    view! { <></> }
}
