# Splinter Proxy

The Splinter proxy is a proxy intended to allow a player to traverse a network of Minecraft servers seamlessly as if they were a single server running a single world.

## Splinter Project

Minecraft servers struggle with large numbers of players. This project would turn a single Minecraft world into a scalable network of servers.

## Usage

### Building Splinter Proxy

You will need Rust. You can get this through [rustup](https://rustup.rs).

Build and run through `cargo run`

### Setting up Minecraft server

Grab a 1.17.1 server from [Spigot BuildTools](https://www.spigotmc.org/wiki/buildtools) or [Paper](https://papermc.io/downloads).

There are some required settings in server.properties:

- `server-port=25400` The server port that Splinter will look for can be changed within its configuration file `config.ron`.
- `online-mode=false` to disable authentication. Splinter cannot use servers with authentication turned on.

Then you can run the server with `java -jar [server jar name].jar --nogui`.

### Joining the proxy

Join with a 1.17.1 Minecraft client. If you're running the proxy on the same device you're playing from, then you can connect to `localhost`.

## Contributing

Join the [OpenClique](https://discord.gg/F93NMyBHda) discord.

Check out the [prototype](https://github.com/OpenCliqueCraft/splinter-prototype).

Contact:

- Discord: [Leap#0765](https://gardna.net/discord)
- Discord: [regenerativep#4103](https://discord.com/users/198652932802084864)

