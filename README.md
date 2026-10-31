# OpenCam

화면을 녹화해 GIF·WebP·APNG·MP4·WebM으로 저장하고 편집하는 Windows 프로그램입니다. (0.1.5)

*Screen recorder and GIF/video editor for Windows. The UI is in Korean.*

## 다운로드

[Releases](https://github.com/JJ-dot-eng/OpenCam/releases) 페이지에서 받으세요.

- `OpenCam_x.y.z_x64-setup.exe`: 설치 파일. 시작 메뉴에 등록되고 "앱 및 기능"에서 지울 수 있습니다.
- `OpenCam_x.y.z_x64_portable.zip`: 설치 없이 압축만 풀어 실행하는 버전.

**필요한 것**

- Windows 10·11 64비트
- Microsoft Edge WebView2 런타임: Windows 11에는 기본으로 들어 있습니다. 설치 파일은 없으면 인터넷으로 받아 함께 설치하고,
  포터블 버전은 미리 설치되어 있어야 합니다.

**처음 실행할 때 "Windows의 PC 보호" 창이 뜹니다.** 코드 서명을 하지 않은 프로그램이라서입니다.
"추가 정보 → 실행"을 누르면 됩니다.

**저장 위치**

- 저장한 GIF·영상: 기본 `사진\OpenCam` 폴더 (설정에서 바꿀 수 있음)
- 설정: `%APPDATA%\OpenCam\settings.json`. 화면 표시용 웹뷰 데이터는 `%LOCALAPPDATA%\org.opencam.app`에 생깁니다.
- 작업 자동 저장: 편집 중인 프레임을 `%LOCALAPPDATA%\OpenCam\recovery`에 임시로 써 둡니다. 정상적으로 끄면 지우고,
  프로그램이 갑자기 꺼졌으면 다음에 켤 때 빈 편집 화면에서 복구하거나 버릴 수 있습니다(최근 3개까지).
  설정 → 일반의 "작업 자동 저장"을 끄면 쓰지 않습니다.
- 포터블 버전도 설정·웹뷰 데이터·작업 자동 저장 폴더는 PC에 남으니, 완전히 지우려면 함께 지우세요.

**업데이트**: 켤 때 GitHub 릴리스에서 새 버전이 있는지 확인하고, 있으면 알려 주며 받는 곳을 열어 줍니다.
받기와 설치는 직접 하면 됩니다(설정은 그대로 남습니다). 설정 → 일반의 "켤 때 새 버전 확인"을 끄면 확인하지 않습니다.

**개인정보**: 위의 새 버전 확인(GitHub에서 최신 버전 번호를 받아 오는 것) 말고는 인터넷에 접속하지 않으며,
화면이나 사용 기록 등 어떤 정보도 밖으로 보내지 않습니다.

## 주요 기능

- **녹화**: 가운데가 뚫린 녹화 틀로 영역 지정, 창 찾아 맞추기, 영역 드래그 선택, 모니터 전체 화면, FPS·커서 녹화 설정,
  일시정지, 녹화 중 현재 화면 PNG 저장, 녹화하면서 MP4로 바로 저장
- **편집**: 프레임 삭제·복사·붙여넣기·끌어서 순서 바꾸기, 홀수·짝수·N번째·비슷한 프레임 지우기, 구간 잘라내기, 시간·속도 조절,
  역재생·왕복, 자르기, 크기·회전, 필터 28종, 텍스트·이미지·말풍선·스티커(등장·퇴장 효과), 그리기, 화면 전환, 줌 애니메이션,
  영상 요약, 고품질 확대(2·3·4배)
- **열기**: GIF·WebP·APNG·동영상·이미지 여러 장, 빈 영상, 콜라주
- **저장**: GIF, WebP, APNG, MP4, WebM, AVIF, PNG 연속 이미지

## 단축키

**녹화 틀**

| 키 | 하는 일 |
|---|---|
| F2 | 녹화 / 정지 (녹화 틀이 떠 있을 때, 설정에서 바꿀 수 있음) |
| F3 | 일시정지 |
| Ctrl+S | 녹화 중 지금 화면을 PNG로 저장 |
| Ctrl+Shift+F | 녹화할 창 찾아 맞추기 |
| Ctrl+Shift+D | 영역 드래그로 선택 |
| 방향키 | 녹화 틀 옮기기 (누르고 있으면 빨라짐) |
| Ctrl+Shift+1~4 / Ctrl+1~4 | 녹화 틀 위치 저장 / 불러오기 |

F2·F3는 OpenCam이 켜져 있는 동안 다른 프로그램보다 먼저 받습니다(예: 탐색기의 F2 이름 바꾸기). 불편하면 설정에서 바꾸세요.

**편집 창**

| 키 | 하는 일 |
|---|---|
| Space | 미리보기 재생 / 정지 |
| ← / → , Home / End | 이전·다음·처음·끝 프레임 (Shift를 같이 누르면 범위 선택) |
| Alt+← / Alt+→ | 선택한 프레임을 앞·뒤로 옮기기 |
| Delete | 선택한 프레임 지우기 |
| Ctrl+A / Esc | 모두 선택 / 선택 해제 |
| Ctrl+C / Ctrl+X / Ctrl+V | 프레임 복사 / 잘라내기 / 붙여넣기 |
| Ctrl+Z | 되돌리기 |
| Ctrl+Y / Ctrl+Shift+Z | 다시 실행 |
| Ctrl+S | 저장 |
| Ctrl+O | 파일 열기 |
| Ctrl+Shift+E | 프레임을 이미지로 내보내기 |
| Ctrl+휠 | 미리보기 확대 / 축소 |
| C · F · D · B | 자르기 · 필터 · 그리기 · 삽입(텍스트·이미지·스티커) |
| Q · X · R | 프레임 지우기(홀수·짝수·비슷한 프레임) · 구간만 남기기 · 크기·회전 |
| T · S · G | 화면 전환 · 영상 요약 · 콜라주 |

## MP4·WebM·AVIF 저장

이 형식은 [ffmpeg](https://ffmpeg.org)가 있어야 저장할 수 있습니다. OpenCam에는 들어 있지 않으니 따로 설치하세요.

- `winget install Gyan.FFmpeg` 처럼 설치해 PATH에 두거나,
- `ffmpeg.exe`를 OpenCam 실행 파일과 같은 폴더에 두면 됩니다.

## 직접 빌드하기

[Node.js](https://nodejs.org)와 [Rust](https://rustup.rs)가 필요합니다.

```
npm install
npm run tauri dev      # 개발용 실행
npm run tauri build    # 설치 파일 만들기 (src-tauri/target/release/bundle)
```

- 테스트: `cd src-tauri && cargo test`, `node --test tests/area-geometry.test.mjs`
- 기능 목록과 동작 규칙은 [SPEC.md](SPEC.md)에 있습니다.

**릴리스 만들기** (관리자용)

1. `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`의 버전을 올립니다.
2. 빌드하고 릴리스 파일을 묶습니다.
   ```
   npm run tauri build
   node scripts/package-release.mjs
   ```
3. `src-tauri/target/release/publish/`의 파일 세 개(설치 파일, 포터블 zip, `SHA256SUMS.txt`)를 `v버전` 태그의
   GitHub 릴리스에 올립니다. 켜져 있는 OpenCam은 최신 릴리스의 태그 번호를 보고 새 버전을 알립니다.

## 라이선스

Copyright (C) 2026 JJ

[GPL-3.0-or-later](LICENSE). 함께 쓰는 부품과 그 라이선스는 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)에,
각 부품의 저작권 표시와 라이선스 전문은 [THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt)에 있습니다.
