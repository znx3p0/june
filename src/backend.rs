use actix::dev::ToEnvelope;
use actix::prelude::*;
use actix::{Actor, Message};
use canary::{err, Channel, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::marker::PhantomData;
use std::result::Result as StdResult;
use std::sync::Arc;
use tokio::sync::Mutex;

pub mod __private {
    pub use actix::Handler;
    pub use actix::Message;
    pub use derive_more::From;
    pub use paste::paste;
    pub use serde::{Deserialize, Serialize};
}

pub mod backend {
    use std::marker::PhantomData;

    use actix::{Actor, Message};
    use serde::{Deserialize, Serialize};

    #[repr(transparent)]
    pub struct Wrapper<T>(T);
    impl<T> Wrapper<T> {
        #[inline]
        pub fn new(t: T) -> Self {
            Wrapper(t)
        }
    }
    impl<T: Serialize> Serialize for Wrapper<T> {
        #[inline]
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            self.0.serialize(serializer)
        }
    }
    impl<'de, T: Deserialize<'de>> Deserialize<'de> for Wrapper<T> {
        #[inline]
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            Ok(Wrapper(T::deserialize(deserializer)?))
        }
    }

    impl<A, M, T> actix::dev::MessageResponse<A, M> for Wrapper<T>
    where
        A: Actor,
        M: Message<Result = T>,
    {
        #[inline]
        fn handle(
            self,
            _ctx: &mut <A>::Context,
            tx: Option<actix::dev::OneshotSender<<M as Message>::Result>>,
        ) {
            if let Some(tx) = tx {
                let _ = tx.send(self.0);
            }
        }
    }

    impl<T> Wrapper<T> {
        #[inline]
        pub fn into_inner(self) -> T {
            self.0
        }
    }
    impl<T: Message> Message for Wrapper<T> {
        type Result = T::Result;
    }
    pub struct InnerWrapper<T, S>(T, PhantomData<S>);
    impl<T, S> InnerWrapper<T, S> {
        #[inline]
        pub fn inner(self) -> T {
            self.0
        }
        #[inline]
        pub fn new(t: T) -> Self {
            Self(t, PhantomData)
        }
    }

    impl<T: Serialize, P> Serialize for InnerWrapper<T, P> {
        #[inline]
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            self.0.serialize(serializer)
        }
    }

    impl<'de, T: Deserialize<'de>, P> Deserialize<'de> for InnerWrapper<T, P> {
        #[inline]
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            Ok(InnerWrapper(T::deserialize(deserializer)?, PhantomData))
        }
    }
}

#[macro_export]
macro_rules! remote {
    (
        $v: vis remote $route: ident ($actor: ident) => {
            $($t: ident),* $(,)?
        }
    ) => {
        $crate::__private::paste! {
            #[derive($crate::__private::Deserialize, $crate::__private::Serialize, $crate::__private::From)]
            $v enum $route {
                $($t ($t) ),*
            }

            const _: () = {
                impl $crate::__private::Message for $route {
                    type Result = [<$route Res>];
                }
                #[derive($crate::__private::Deserialize, $crate::__private::Serialize, $crate::__private::From)]
                $v enum [<$route Res>] {
                    $($t ($crate::backend::backend::InnerWrapper<<$actor as $crate::__private::Handler<$t>>::Result, $t>) ),*
                }

                $(
                    impl Into<::std::result::Result<(<$actor as $crate::__private::Handler<$t>>::Result, ::core::marker::PhantomData<$t>), [<$route Res>]>> for [<$route Res>] {
                        #[inline]
                        #[allow(unreachable_code)]
                        fn into(self) -> ::std::result::Result<(<$actor as $crate::__private::Handler<$t>>::Result, ::core::marker::PhantomData<$t>), [<$route Res>]> {
                            match self {
                                [<$route Res>]::$t(inner) => Ok((inner.inner(), PhantomData)),
                                #[allow(unreachable_patterns)]
                                e => Err(e),
                            }
                        }
                    }
                )*
                impl $crate::__private::Message for [<$route Res>] {
                    type Result = (); // satisfy the compiler >:(
                }
                impl $crate::__private::Handler<$route> for $actor {
                    type Result = $crate::backend::backend::Wrapper<[<$route Res>]>;

                    #[allow(unreachable_code)]
                    #[inline]
                    fn handle(&mut self, msg: $route, ctx: &mut Self::Context) -> Self::Result {
                            $crate::backend::backend::Wrapper::new(match msg {
                            $($route::$t(arg) => [<$route Res>]:: $t ($crate::backend::backend::InnerWrapper::new(self.handle(arg, ctx)))),*
                        })
                    }
                }
                impl $crate::Route<$actor> for $route {
                    type Message = Self;
                    type Result = [<$route Res>];
                }
            };
        }
    };
}

#[cfg(feature = "maquio-routing")]
pub trait RemoteActor: Actor {
    fn route_from_addr<N: Route<Self>>(addr: actix::Addr<Self>) -> maquio::router::Service
    where
        Self: Handler<N::Message>,
        N::Message: Send + Sync,
        <N::Result as Message>::Result: Send + Sync,
        <<N as Route<Self>>::Message as Message>::Result: Send + Sync + Serialize,
        <Self as Actor>::Context: ToEnvelope<Self, N::Message>,
        <N as Route<Self>>::Message: Message,
        <N as Route<Self>>::Result: Message,
    {
        {
            From::from(move |mut c: Channel| {
                let addr = addr.clone();
                async move {
                    loop {
                        let msg = c.receive::<N::Message>().await?;
                        let res = addr.send(msg).await.map_err(|e| err!(e))?;
                        c.send(res).await?;
                    }
                }
            })
        }
    }
}

/// exposes an actor through a channel.
/// ```norun
/// remote! {
///     pub remote MyRemote(MyActor) {
///         Increase, Decrease
///     }
/// }
///
/// expose_actor::<MyActor, MyRemote>(&addr, &mut chan).await?;
/// ```
pub async fn expose_actor<A, R>(
    addr: &actix::Addr<A>,
    chan: &mut Channel,
) -> Result<core::convert::Infallible>
where
    A: Actor + Handler<R::Message>,
    R: Route<A>,
    <R as Route<A>>::Message: Message,
    <R as Route<A>>::Result: Message,
    <<R as Route<A>>::Message as Message>::Result: Send + Sync + 'static,
    <A as Actor>::Context: ToEnvelope<A, <R as Route<A>>::Message>,
    <<R as Route<A>>::Message as Message>::Result: Serialize,
{
    loop {
        let msg = chan.receive::<R::Message>().await?;
        let res = addr.send(msg).await.map_err(|e| err!(e))?;
        chan.send(res).await?;
    }
}

#[cfg(feature = "maquio-routing")]
impl<T: Actor> RemoteActor for T {}

pub trait Route<A: Actor> {
    type Message: Serialize + DeserializeOwned + Send + 'static;
    type Result: Serialize + DeserializeOwned + Send + 'static;
}

#[cfg(feature = "maquio-routing")]
pub trait AddrSvc<A: Actor> {
    fn service<N: Route<A>>(self) -> maquio::router::Service
    where
        <A as Actor>::Context: ToEnvelope<A, <N as Route<A>>::Message>,
        <N as Route<A>>::Message: Message,
        A: Handler<<N as Route<A>>::Message>,
        <N as Route<A>>::Result: Message,
        <<N as Route<A>>::Message as Message>::Result: Send + Sync + Serialize,
        <N as Route<A>>::Message: Send + Sync,
        <<N as Route<A>>::Result as Message>::Result: Send + Sync;
}

#[cfg(feature = "maquio-routing")]
impl<A: Actor> AddrSvc<A> for actix::Addr<A> {
    fn service<N: Route<A>>(self) -> maquio::router::Service
    where
        <A as Actor>::Context: ToEnvelope<A, <N as Route<A>>::Message>,
        <N as Route<A>>::Message: Message,
        A: Handler<<N as Route<A>>::Message>,
        <N as Route<A>>::Result: Message,
        <N as Route<A>>::Message: Send + Sync,
        <<N as Route<A>>::Result as Message>::Result: Send + Sync,
        <<N as Route<A>>::Message as Message>::Result: Send + Sync + Serialize,
    {
        A::route_from_addr::<N>(self)
    }
}

pub enum Addr<A: Actor, N: Route<A>> {
    Local(actix::Addr<A>),
    Remote(Arc<Mutex<Channel>>, PhantomData<N>),
}

impl<A: Actor, N: Route<A>> Clone for Addr<A, N> {
    fn clone(&self) -> Self {
        match self {
            Self::Local(addr) => Self::Local(addr.clone()),
            Self::Remote(arc, ..) => Self::Remote(arc.clone(), PhantomData),
        }
    }
}

impl<A: Actor, N: Route<A>> From<actix::Addr<A>> for Addr<A, N> {
    fn from(addr: actix::Addr<A>) -> Self {
        Addr::Local(addr)
    }
}
impl<A: Actor, N: Route<A>> From<Channel> for Addr<A, N> {
    fn from(chan: Channel) -> Self {
        Addr::Remote(Arc::new(Mutex::new(chan)), PhantomData)
    }
}

impl<A: Actor, N: Route<A>> Addr<A, N> {
    pub fn new(addr: impl Into<Addr<A, N>>) -> Self {
        addr.into()
    }
    pub async fn send<M>(&self, msg: M) -> Result<M::Result>
    where
        M: Message + Send + 'static,
        N::Message: From<M>,
        M::Result: Send + DeserializeOwned,
        A: Handler<M>,
        A::Context: ToEnvelope<A, M>,
        N::Result: Into<StdResult<(M::Result, PhantomData<M>), N::Result>>,
    {
        match self {
            Addr::Local(addr) => addr.send(msg).await.map_err(|e| err!(e)),
            Addr::Remote(chan, ..) => {
                let mut chan = chan.lock().await;
                chan.send(N::Message::from(msg)).await?;
                let res = chan.receive::<N::Result>().await?;
                let res: StdResult<(M::Result, PhantomData<M>), N::Result> = res.into();
                res.map(|val| val.0).map_err(|_| err!("unexpected type"))
            }
        }
    }
}
