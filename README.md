# image-server

현재는 `image-server-cli`로 서버 런타임과 room store를 다룰 수 있습니다.

## CLI

예시 config:

```bash
./examples/server-config.toml
```

기본 실행 형식:

```bash
cargo run -p image-server-cli -- --config ./examples/server-config.toml <command>
```

### 서버 실행

```bash
cargo run -p image-server-cli -- --config ./examples/server-config.toml serve
```

현재 `serve`는 runtime만 올리고 대기합니다.
아직 watcher와 HTTP ingress는 붙어 있지 않습니다.

### 상태 조회

```bash
cargo run -p image-server-cli -- --config ./examples/server-config.toml status
cargo run -p image-server-cli -- --config ./examples/server-config.toml status --json
```

### room 목록

```bash
cargo run -p image-server-cli -- --config ./examples/server-config.toml room list
cargo run -p image-server-cli -- --config ./examples/server-config.toml room list --json
```

### room 생성

```bash
cargo run -p image-server-cli -- --config ./examples/server-config.toml room create \
  --id room-a \
  --name "Room A" \
  --target-path ./samples/room-a.clip
```

추가 옵션:

```bash
--mode watch|interval
--interval-ms 2000
--debounce-ms 750
--stabilize-ms 300
--resolution source|contain
--max-width 1440
--max-height 1440
--json
```

### room 수정

```bash
cargo run -p image-server-cli -- --config ./examples/server-config.toml room update \
  --id room-a \
  --name "Room A Updated" \
  --mode interval
```

예시:

```bash
cargo run -p image-server-cli -- --config ./examples/server-config.toml room update \
  --id room-a \
  --resolution contain \
  --max-width 1024 \
  --max-height 768
```

### room 삭제

```bash
cargo run -p image-server-cli -- --config ./examples/server-config.toml room delete --id room-a
```

## 예시 파일

- server config: `examples/server-config.toml`
- room store: `examples/image-server-store.toml`
