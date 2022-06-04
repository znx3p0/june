# June

June is a library that enables remote actors designed for use in distributed systems.

It is designed to be as non-obstructive as possible, and its developer experience is
extremely similar to using raw actix (since it uses actix for the heavy-lifting).

There are just a few things one needs to know to start using remote actors.

```rust
// everything in this specific part is what you would normally do
// while working with actix. In case of any doubt in this part, you should
// look at the docs from actix.

// we'll first define a normal actix actor
struct MyActor {
    count: u64,
}
// declare the actor and its context
impl Actor for MyActor {
    type Context = Context<Self>;
}

// Declare `Increase` message.
// Messages need to implement Serialize and Deserialize
// to be able to be sent and received in remote actors.
#[derive(Message, Serialize, Deserialize)]
#[rtype(result = "u64")]
struct Increase(u64);

/// Handler for `Increase` message
impl Handler<Increase> for MyActor {
    type Result = u64;

    fn handle(&mut self, msg: Increase, _: &mut Context<Self>) -> Self::Result {
        self.count += msg.0;
        println!("Increased. Count: {}", self.count);
        self.count
    }
}


// Declare `Reduce` message
#[derive(Message, Serialize, Deserialize)]
#[rtype(result = "u64")]
struct Reduce(u64);

/// Handler for `Reduce` message
impl Handler<Reduce> for MyActor {
    type Result = u64;

    fn handle(&mut self, msg: Reduce, _c: &mut Self::Context) -> Self::Result {
        self.count -= msg.0;
        println!("decreased. count: {}", self.count);
        self.count
    }
}
```

After declaring the actor and its messages, we can use it remotely via june.

```rust
// start by declaring a remote, which is a wrapper that encapsulates all messages
// that can be sent remotely.
remote! {
    remote MyRoute (MyActor) => { // <vis> remote <name> (<actor name>) => { <all messages> }
        Reduce,
        Increase,
    }
}

// now we can expose an actor
async fn server() -> Result<()> {
    let addr = MyActor { count: 0 }.start();

    let tcp = canary::Tcp::bind("127.0.0.1:8080").await?;
    while let Ok(hs) = tcp.next().await {
        let addr = addr.clone();
        tokio::spawn(async move {
            let mut chan = hs.encrypted().await.unwrap();
            june::expose_actor::<_, MyRoute>(&addr, &mut chan)
                .await
                .ok();
        });
    }
    Ok(())
}

// and we can now dial the actor we exposed
async fn client() -> Result<()> {
    let chan: canary::Channel = Tcp::connect("127.0.0.1:8080").await?.encrypted().await?;
    let actor: Addr<MyActor, MyRoute> = Addr::new(chan);
    let res = actor.send(Increase(10)).await?;
    Ok(())
}
```

June also offers a `maquio-routing` feature that enables traits for working with `maquio` routers more ergonomically.
