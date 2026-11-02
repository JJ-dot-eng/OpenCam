# 인계

갱신: 2026-11-02, Claude Code 담당

자세한 기록은 [docs/TODO.md](docs/TODO.md).

## 끝난 일

- PR #4·#5를 main에 합치고 0.1.7로 릴리스함.
- PR #6(GIF 투명 잔상)·#7(붙이기 겹침·작업 변경)·#8(동영상 일부만 읽음 경고·길이 없는 동영상)을 main에 합치고 0.1.8로 릴리스함.
- PR #9(끌어 놓기·붙이기 잠금 노드 테스트)·#10(창 없는 Rust 미디어 e2e 테스트)·#11(동영상 붙이기 메모리 한도)을 main에 합치고 0.1.9로 릴리스함.
- GitHub Actions CI가 PR마다 Windows에서 타입 검사·노드 테스트·화면 빌드·Rust 빌드·cargo test를 돌린다.
- PR #13(편집 중 썸네일 요청 때문에 앱이 멈출 수 있던 문제)을 main에 합침(main 커밋 fa02b7c). Windows CI 통과(cargo test 123개, 노드 테스트 49개). 실제 앱 확인은 아직.
- PR #14(프레임 시간 오류 3개: APNG 긴 프레임, APNG·WebP 짧은 시간 보정, 시간 합 넘침)를 main에 합침(main 커밋 f736b7b). Windows CI 통과(cargo test 131개, 노드 테스트 49개). 실제 앱 확인은 아직.
- PR #15(다른 worktree에서는 여러 AI가 병렬로 작업해도 되게 AGENTS.md 규칙 바꿈, 사용자 승인)를 main에 합침(main 커밋 3d637cc).
- PR #16(남은 시간 합 넘침: `project.rs`의 `drop_frames`·`summary_plan`·`zoom_fx`를 u64로 셈)을 main에 합침(main 커밋 090eb7f). Windows CI 통과(cargo test 134개). 실제 앱 확인은 아직.
- 브랜치 `fix/insert-blob-url`(이 PR): 텍스트·이미지 삽입에서 파일·클립보드로 넣은 그림의 Blob URL을 읽기 성공·실패 때 한 번만 해제하고, 삽입 화면이 닫히면 읽는 중인 것도 해제하며 그 뒤 온 결과는 버린다. 해제 로직은 `src/blobImage.ts`로 빼고 노드 테스트 `tests/blobImage.test.mjs` 6개를 더함. Linux Node 22에서 `npx tsc --noEmit`과 노드 테스트 55개 통과. Rust는 바꾸지 않았고 이 PC에서 빌드하지 않음. Windows CI 결과는 PR 참고.
- 진행률 99% 정체는 조사만 하고 코드는 고치지 않음(원인이 분명하지 않음). 아래 "남은 일" 참고.

## 남은 일

- 이번 PR(`fix/insert-blob-url`)을 합칠지 사용자가 정한다. 합친 뒤 Windows PC에서 삽입 화면의 "이미지 파일…"·"클립보드 이미지"로 그림을 넣고 미리보기·크기·회전·적용이 예전처럼 되는지, 그림을 읽는 중에 삽입 화면을 닫아도 오류가 뜨지 않는지 본다.
- PR #16 합친 뒤 확인: Windows PC에서 아주 긴 프레임이 있는 녹화로 "속도 유지" 프레임 지우기·영상 요약·줌 효과를 실제 앱에서 본다.
- PR #14 합친 뒤 확인: Windows PC에서 프레임 시간 120초 APNG 저장·다시 열기, 10ms APNG·WebP 열기, 아주 긴 프레임 합치기를 실제 앱에서 본다.
- PR #13 합친 뒤 확인: Windows PC에서 큰 녹화를 편집(크기 바꾸기·필터)하는 동안 썸네일을 빠르게 스크롤해 멈추지 않는지 본다.
- Windows PC에서 [docs/real-app-checklist.md](docs/real-app-checklist.md) 항목을 확인한다. 세로 모니터 녹화, 복구 카드의 복구 동작, 0.1.7의 GIF·PNG 저장, 0.1.9의 비율이 다른 동영상 붙이기도 실제 앱 확인이 남아 있다.
- 낮음 후보: 진행률 99% 정체. 조사 결과(코드는 그대로):
  - 99%에서 멈추는 곳은 동영상 불러오기의 `src-tauri/src/import.rs` `collect_raw_frames`(약 418줄, `progress((k / expected).min(0.99))`) 하나다. `expected`는 `load_video_inner`(약 540줄)에서 구간 길이 × FPS로 센다.
  - 가설 1: ffmpeg가 `expected`보다 많은 프레임을 내보내면(길이 정보가 실제보다 짧음, 가변 FPS 등) 남은 프레임을 읽는 동안 99%에 머문다.
  - 가설 2: 다 읽은 뒤 ffmpeg 종료 기다리기와 화면 쪽 다시 읽기 동안 마지막 글자 "… 99%"가 그대로 보인다(`src/Editor.tsx` 약 208줄 `import-progress`). `Project::load`는 가벼워 원인이 아님.
  - 붙이기(`src-tauri/src/lib.rs` `append_files`, 약 1711줄)는 파일마다 (k + 0.99)/n까지 오른다. 저장은 99%에 멈추지 않는다(동영상 저장은 95%에서 ffmpeg를 기다리고, WebP는 100% 뒤 `finalize`가 길 수 있음).
  - 다음: 실제 앱에서 어떤 동영상(길이·FPS·형식)으로 얼마나 오래 멈추는지 먼저 본 뒤 고칠지 정한다.
- 비슷한 위험(고치지 않음, 지금은 이미지 요청 외에 Rayon 안에서 프로젝트 잠금을 잡는 곳이 없어 멈추지 않음): `trim_bounds` 명령·`summary_preview`·`apply_overlay`도 잠금을 쥔 채 Rayon을 기다린다. 나중에 Rayon 안에서 프로젝트 잠금을 잡는 코드를 넣지 않도록 주의.
- 보류한 버그: 정전 시 복구 파일 동기화, 녹화 틀 옮길 때 잠깐 끊김, 저장 결과에 단축키 오류.
- AGENTS.md의 Rust 테스트 수(75)는 오래되었다(main 134개). 사용자 허락 없이 고치지 않음.

## 사용자가 정한 것
- 새 기능은 바로 추가하지 않고 직접 사용하거나 사용자 반응을 본 뒤 정한다.
- 할 일과 기획 문서는 공개 저장소 docs에 둔다. 여러 AI는 AGENTS.md의 협업 규칙을 따른다(다른 worktree·브랜치라면 병렬 작업 가능, 한 worktree에는 한 AI만).
- 코드 서명은 하지 않는다.
- 끌어 놓은 동영상 여러 개도 파일 이름 순서(숫자는 크기대로)로 맨 뒤에 붙인다. 편집 중이 아니면 기존처럼 연다.
- 새 브랜치와 PR로만 올리고 main에 직접 push·합치기·강제 push 하지 않는다.
