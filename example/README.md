# How to play with drill

Compile drill:

```
cargo build --release
```

### Example 1

Start a HTTP server from `responses` directory in another terminal:

```
cd example/responses
python server.py
```

and then run:

```
cd example
../target/release/drill --benchmark benchmark.yml
```

### Example 2

Start a Node HTTP server from `server` directory in another terminal:

```
cd example/server
npm install
node server.js
```

and then run:

```
cd example
../target/release/drill --benchmark cookies.yml
```

### Example 3

Start a Node HTTP server from `server` directory in another terminal:

```
cd example/server
npm install
node server.js
```

and then run:

```
cd example
../target/release/drill --benchmark headers.yml
