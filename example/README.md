# How to play with drill

Compile drill:

```
cargo build --release
```

Start a HTTP server from `responses` directory in another terminal:

```
cd example/responses
python -m SimpleHTTPServer 9000
```

and then run:

```
cd example
../target/release/drill --benchmark benchmark.yml
```
