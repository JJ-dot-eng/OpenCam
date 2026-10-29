# OpenCam

화면을 녹화해 GIF·WebP·APNG·MP4·WebM으로 저장하고 편집하는 Windows 프로그램입니다. (0.1.0)

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

## 주요 기능

- **녹화**: 가운데가 뚫린 녹화 틀로 영역 지정, 창 찾아 맞추기, 영역 드래그 선택, 모니터 전체 화면, FPS·커서 녹화 설정,
  일시정지, 녹화 중 현재 화면 PNG 저장, 녹화하면서 MP4로 바로 저장
- **편집**: 프레임 삭제·복사·붙여넣기·끌어서 순서 바꾸기, 홀수·짝수·N번째·비슷한 프레임 지우기, 구간 잘라내기, 시간·속도 조절,
  역재생·왕복, 자르기, 크기·회전, 필터 28종, 텍스트·이미지·말풍선·스티커(등장·퇴장 효과), 그리기, 화면 전환, 줌 애니메이션,
  영상 요약, 고품질 확대(2·3·4배)
- **열기**: GIF·WebP·APNG·동영상·이미지 여러 장, 빈 영상, 콜라주
- **저장**: GIF, WebP, APNG, MP4, WebM, AVIF, PNG 연속 이미지

## 단축키

| 키 | 하는 일 |
|---|---|
| F2 | 녹화 / 정지 (녹화 틀이 떠 있을 때, 설정에서 바꿀 수 있음) |
| F3 | 일시정지 |
| Space | 미리보기 재생 / 정지 |
| Ctrl+Z | 되돌리기 |
| Ctrl+S | 저장 |
| Ctrl+O | 파일 열기 |
| Ctrl+휠 | 미리보기 확대 / 축소 |

F2·F3는 OpenCam이 켜져 있는 동안 다른 프로그램보다 먼저 받습니다(예: 탐색기의 F2 이름 바꾸기). 불편하면 설정에서 바꾸세요.

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

## 라이선스

[GPL-3.0-or-later](LICENSE). 함께 쓰는 부품과 그 라이선스는 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)에 있습니다.
