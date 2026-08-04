# gRPC support implementation plan

## Goal

Add native gRPC unary-call support to Drill so it can load-test gRPC services the same way it currently load-tests HTTP services.

## Scope

- **Phase 1 (this implementation):** unary calls only, schema loaded from a pre-generated protobuf descriptor set file.
- **Phase 2 (optional):** gRPC server reflection to fetch descriptors at runtime.
- **Phase 3 (optional):** client/server/bidirectional streaming.

## Dependencies

Add to `Cargo.toml`:

- `tonic` — HTTP/2 gRPC transport.
- `prost` — Protobuf encoding/decoding.
- `prost-reflect` — runtime protobuf reflection, dynamic messages.
- `prost-types` — well-known protobuf types.
- `bytes` — byte buffers passed through the custom codec.

## User workflow

1. Generate a descriptor set file from the service `.proto`:

   ```bash
   protoc --descriptor_set_out=service.pb \
          --include_imports \
          -I ./proto \
          ./proto/helloworld.proto
   ```

2. Reference the descriptor set and call the service in a benchmark:

   ```yaml
   - name: Greet
     grpc:
       descriptor_set: ./proto/service.pb
       service: helloworld.Greeter
       method: SayHello
       body:
         name: "World"
       metadata:
         authorization: "Bearer {{ token }}"
     assign: response
   ```

3. The response is stored as JSON in the context, so later steps can interpolate `{{ response.body.message }}`.

## Architecture

### Runtime schema loading

- `Grpc` action holds the path to the descriptor set file.
- On first use (or lazily at parse time), load the file into a `prost_reflect::DescriptorPool`.
- Look up the service descriptor by name, then the method descriptor.
- Use `MethodDescriptor::input()` and `output()` to get the request and response message descriptors.

### JSON ↔ protobuf transcoding

- Use `prost-reflect` with its `serde` feature enabled.
- Request: deserialize the YAML/JSON body into a `DynamicMessage` using `DynamicMessage::deserialize(input_descriptor, &mut json_deserializer)`.
- Encode the `DynamicMessage` to protobuf bytes.
- Response: decode the protobuf bytes into a `DynamicMessage` using `DynamicMessage::decode(output_descriptor, response_bytes)`.
- Serialize the response `DynamicMessage` to JSON using `serde_json::to_string`.

### Tonic transport

- Build a `tonic::transport::Channel` from the benchmark `base` URL.
- Create a `tonic::client::Grpc<Channel>` instance.
- Implement a custom `tonic::codec::Codec` where both `Encode` and `Decode` are raw `Bytes`. The codec does not interpret the bytes; it only forwards them.
- Call `Grpc::unary(request, path, codec).await` where the path is `/fully.qualified.Service/Method`.

### Metadata/headers

- Support `metadata` as a map of strings, resolved through the interpolator.
- Pass them as `tonic::metadata::MetadataMap` on the outgoing request.

### TLS

- Reuse the existing `--no-check-certificate` flag.
- If the scheme is `https`, configure Tonic's TLS with `ClientTlsConfig` and disable certificate verification when the flag is set.

### Statistics

- Produce a `Report` for each gRPC call, similar to HTTP requests:
  - `duration` measured end-to-end.
  - `status` mapped from the gRPC status code (e.g., `0` for OK, otherwise the status code integer).
  - `timestamp` of the call.

## Files to change

| File | Change |
| ---- | ------ |
| `Cargo.toml` | Add `tonic`, `prost`, `prost-reflect`, `prost-types`, `bytes`. |
| `src/actions/grpc.rs` | New `Grpc` action with descriptor loading, transcoding, and unary call. |
| `src/actions/mod.rs` | Export `Grpc`; add it to the `Runnable` ecosystem. |
| `src/expandable/include.rs` | Detect `grpc:` items and expand them into the benchmark. |
| `src/benchmark.rs` | No structural changes expected; `Grpc` implements `Runnable`. |
| `src/actions/grpc.rs` (tests) | Unit tests for descriptor loading, JSON transcoding, and path construction. |
| `SYNTAX.md` | Document the `grpc` action and descriptor set generation. |
| `README.md` | Add gRPC to the feature list. |

## YAML schema

```yaml
grpc:
  descriptor_set: string          # path to .pb descriptor set file
  service: string                 # fully qualified service name
  method: string                  # method name
  body: any                       # JSON-serializable request body
  metadata: {string: string}       # optional gRPC metadata
```

## Risks and open questions

1. **Schema source:** Start with descriptor set files only? Server reflection is useful but requires a separate discovery step.
2. **TLS configuration:** Reuse `--no-check-certificate` or add a dedicated `tls` field?
3. **Body format:** Plain JSON for consistency with the rest of Drill; protobuf text format can be added later.
4. **Error mapping:** gRPC status codes are not HTTP status codes. Decide how to surface them in reports and stats.
5. **Streaming:** Out of scope for the first pass.

## Recommended first step

Implement unary calls with descriptor set files and JSON bodies, then add an end-to-end test against a small tonic example server.
