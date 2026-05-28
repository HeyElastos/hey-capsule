use leptos::prelude::*;

#[component]
pub fn Landing() -> impl IntoView {
    view! { <Stub label="Landing" /> }
}

#[component]
pub fn SignIn() -> impl IntoView {
    view! { <Stub label="Sign In" /> }
}

#[component]
pub fn SignUp() -> impl IntoView {
    view! { <Stub label="Sign Up" /> }
}

#[component]
pub fn Onboarding() -> impl IntoView {
    view! { <Stub label="Onboarding" /> }
}

#[component]
pub fn Home() -> impl IntoView {
    view! { <Stub label="Home (photo feed)" /> }
}

#[component]
pub fn Clips() -> impl IntoView {
    view! { <Stub label="Clips (video feed)" /> }
}

#[component]
pub fn PostDetail() -> impl IntoView {
    view! { <Stub label="Post detail" /> }
}

#[component]
pub fn VideoPlayer() -> impl IntoView {
    view! { <Stub label="Video player (Elacity-backed)" /> }
}

#[component]
pub fn Profile() -> impl IntoView {
    view! { <Stub label="Profile" /> }
}

#[component]
pub fn Chat() -> impl IntoView {
    view! { <Stub label="Chat" /> }
}

#[component]
pub fn NotFound() -> impl IntoView {
    view! { <Stub label="404" /> }
}

#[component]
fn Stub(label: &'static str) -> impl IntoView {
    view! {
        <section style="padding: 2rem; font-family: system-ui">
            <h1 style="font-size: 1.5rem; font-weight: 600">{label}</h1>
            <p style="opacity: 0.6">"Placeholder — port from capsules/hey-social/client/src/."</p>
        </section>
    }
}
