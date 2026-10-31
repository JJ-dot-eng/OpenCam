# AI 작업 안내

이 저장소에서 일하는 AI 도구(코딩 에이전트)가 먼저 읽는 파일이다. 사람용 소개는 [README.md](README.md).

## 먼저 읽을 것

- [docs/TODO.md](docs/TODO.md): 지금까지 한 일, 보류한 버그, 다음 방향. **가장 최신 상태는 여기.**
- [docs/next-features.md](docs/next-features.md): 다음 기능 후보와 화면 배치안 (2026-10-29 작성, 맨 위 "지금 상태" 참고)
- [SPEC.md](SPEC.md): 기능 전체 목록과 동작 규칙

## 프로젝트

Windows용 화면 녹화·GIF 편집 프로그램. Tauri 2 (Rust) + React 19 (TypeScript) + Vite.

- `src/`: 화면(React). 창마다 같은 `index.html`을 쓰고 주소의 `#`으로 화면을 고른다(`main.tsx`).
  편집 창 `Editor.tsx`, 녹화 틀 `Recorder.tsx`, 영역 선택 `AreaSelector.tsx`.
- `src-tauri/src/`: 캡처·녹화·편집·저장(Rust). 화면에서 부르는 명령은 `lib.rs`, 화면 쪽 호출은 `src/api.ts`.
- `src-tauri/models/`: 초해상도(ESPCN) 가중치. `superres.rs`가 직접 계산한다.
- `scripts/`: 릴리스 묶기, 부품 라이선스 모으기.

## 실행·테스트

```
npm install
npm run tauri dev                          # 개발용 실행
cd src-tauri && cargo test                 # Rust 테스트 75개 (ffmpeg가 있으면 MP4·WebM도 실제로 만든다)
node --test tests/area-geometry.test.mjs   # 영역 좌표 계산 (Node.js 24)
npx tsc --noEmit                           # 화면 코드 타입 검사
```

## 지킬 것

- 화면 글자, 주석, 문서, 커밋 메시지는 **쉬운 한국어**로 쓴다. 주변 코드의 말투와 주석 양을 따른다.
- 파일·주석·커밋에 **다른 화면 녹화 프로그램의 이름을 쓰지 않는다.** 비교가 필요하면 저장소 밖에서 한다.
- 확인·입력 창은 `window.confirm`·`prompt`를 쓰지 말고 `src/ask.tsx`의 `confirmAsk`·`promptAsk`·`alertAsk`를 쓴다.
  이 앱의 웹뷰에서는 `window.confirm`이 창을 띄우지 않고 바로 "예"를 돌려준다.
- 버전을 올릴 때는 `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` 세 곳을 같이 바꾼다. 릴리스 순서는 README "릴리스 만들기".
- 일을 마치면 [docs/TODO.md](docs/TODO.md)에 한 일, 실제 앱에서 확인한 것과 못 한 것을 적는다.

## 알아 둘 것

- 개발 서버를 강제로 끄면 Vite(node)가 1420번 포트를 잡은 채 남을 수 있다. 다음 실행이 "Port 1420 is already in use"로 실패하면 그 node 프로세스를 끄면 된다.
- F2/F3는 전역 단축키라서 OpenCam이 켜져 있으면 다른 프로그램의 F2(예: 탐색기 이름 바꾸기)를 가져간다.
- 되돌리기는 최대 50단계까지 프레임 목록을 통째로 기억한다. 필터처럼 픽셀을 바꾸는 편집을 큰 녹화에 여러 번 하면 메모리를 많이 쓸 수 있다.
- 필터 미리보기는 원본 크기로 PNG를 만들어 보여준다. 아주 큰 녹화에서는 미리보기가 느릴 수 있다.
- `cargo build`와 `tauri dev`가 동시에 빌드하면 빌드 파일이 깨져 링크 오류(LNK2019 수천 개)가 날 수 있다.
  그러면 개발 서버를 끄고 `cargo clean -p tauri -p tauri-runtime-wry -p opencam` 후 다시 빌드.
- 개발 중 실시간 반영(HMR)이 아주 빠른 연속 저장을 놓치면 옛 코드가 남는다. 이상하면 파일을 다시 저장(touch)하고 편집 창에서 F5.
- `cargo clippy`는 `chunks_exact` → `as_chunks` 같은 스타일 제안만 낸다(오류 아님).
