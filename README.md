# Splinter Proxy

The Splinter proxy is an advanced proxy intended to allow a player to traverse the network of Splinter Minecraft servers seamlessly. We do not use BungeeCord for this task because BungeeCord's abilities are limited and the JVM can be slower than target specific compiled code.

## Splinter Project

Minecraft servers have a problem: they are all single-threaded. Even if a Minecraft server were to be multithreaded, it would still be limited by the single processor that runs it. The project aims to turn a single Minecraft survival world into a distributed network of Minecraft servers, potentially making the player limit in a single world proportional to the amount of hardware.

## Usage

### Building Splinter Proxy

You will need Rust. You can get this through [rustup](https://rustup.rs).

You need to be on the nightly branch for certain features. `rustup default nightly`

Build and run through `cargo run`

### Setting up Minecraft server

Grab a 1.16.5 server from [Spigot BuildTools](https://www.spigotmc.org/wiki/buildtools) or [Paper](https://papermc.io/downloads).

There are some required settings in server.properties:

- `server-port=25400` Server port can be changed within the config file of the proxy.
- `online-mode=false` to disable authentication, as authentication will be done either in the proxy or something on top of it like BungeeCord.

Then you can run the server with `java -jar [server jar name].jar --nogui` or run it as a normal application.

### Joining the proxy

Join with a 1.16.5 Minecraft client. If you're running the proxy on the same device you're playing from, then you can connect to `localhost:25565`.

## Contributing

Join the [OpenClique](https://discord.gg/F93NMyBHda) discord.

Check out the [prototype](https://github.com/OpenCliqueCraft/splinter-prototype).

Contact:

- Discord: [Leap#0765](https://gardna.net/discord)
- Discord: [regenerativep#4103](https://discord.com/users/198652932802084864)

