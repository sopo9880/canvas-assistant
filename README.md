# Canvas Assistant

Canvas LMS와 Codex(ChatGPT)를 연결한 Windows용 개인 학습 비서입니다. 과제 마감, 공지, 성적을 한 화면에서 보고, AI에게 물어보면 실제 Canvas 데이터를 조회해서 답합니다.

<p align="center"><img src="public/logo.png" width="96" alt="Canvas Assistant"></p>

## 주요 기능

- **AI 대화**: "이번 주 마감 과제 정리해줘", "최근 공지 요약해줘"처럼 물어보면 Codex가 Canvas를 직접 조회해서 답합니다. 마크다운 표·목록을 지원하고 대화 기록이 남습니다.
- **Canvas 대시보드**: 다가오는 과제(남은 시간·제출 여부), 강의별 현재 점수, 최근 7일 공지를 한눈에 봅니다. 강의마다 과제·공지·모듈·파일·성적·토론 탭이 있고, 파일은 `다운로드\Canvas Assistant` 폴더로 받습니다.
- **승인 대기**: AI는 Canvas를 직접 바꾸지 않습니다. 과제 제출, 토론 댓글, 할 일·일정 추가는 요청으로만 올라오고, 앱에서 승인해야 실행됩니다. 모든 실행은 활동 기록에 남습니다.
- **자동화와 알림**: 마감 임박(기본 24시간·3시간 전), 새 공지, 새 성적을 Windows 알림으로 알려줍니다. 원하면 매일 정한 시각에 AI 브리핑을 받을 수 있습니다.
- **트레이·자동 시작**: 창을 닫아도 트레이에서 계속 동작하고, Windows 시작 시 자동 실행할 수 있습니다.

## 설치

1. [Releases](../../releases)에서 `CanvasAssistant-x.y.z-x64-setup.exe`를 받아 실행합니다. 관리자 권한 없이 현재 사용자에게 설치됩니다.
2. 코드 서명이 없어서 처음 실행할 때 "Windows의 PC 보호" 창이 뜨면 **추가 정보 → 실행**을 누르세요.
3. Codex는 앱에 포함되어 있어서 따로 설치할 필요가 없습니다.

요구 사항: Windows 10/11 64비트, WebView2(설치 프로그램이 없으면 자동 설치), ChatGPT 계정(AI 기능), 학교 Canvas 계정.

## 처음 설정

1. **설정 → Canvas 연결**에 학교 Canvas 주소(예: `canvas.학교.ac.kr`)를 입력합니다.
2. **토큰 발급 페이지 열기**를 눌러 Canvas의 *계정 → 설정 → 새 액세스 토큰*에서 토큰을 만들고 붙여넣은 뒤 **연결**을 누릅니다. 연결을 확인한 뒤에만 저장됩니다.
3. **설정 → ChatGPT 계정**(또는 AI 대화 화면)에서 **ChatGPT로 로그인 (브라우저)** 를 누릅니다. 브라우저 로그인이 안 되면 **기기 코드로 로그인**을 쓰세요.

## 보안과 개인정보

- Canvas 토큰은 **Windows 자격 증명 관리자**에만 저장됩니다. 설정 파일·로그·화면에 남지 않고, 토큰은 설정한 Canvas 주소로만 전송됩니다(다른 도메인으로 가는 페이지 링크나 리다이렉트에는 보내지 않음).
- ChatGPT 로그인은 앱에 포함된 Codex가 처리하고, 로그인 정보는 Windows 자격 증명 관리자에 저장됩니다. Codex는 앱 전용 폴더(`%APPDATA%\com.sopo9880.canvasassistant\codex`)를 써서 PC에 따로 설치된 Codex와 섞이지 않습니다.
- AI는 **읽기 전용 샌드박스**에서 실행되고 셸 도구·웹 검색이 꺼져 있습니다. 명령 실행·파일 수정 요청은 항상 거절되고, AI가 쓸 수 있는 도구는 Canvas 조회 도구와 '변경 제안' 도구뿐입니다.
- Canvas를 바꾸는 작업은 모두 **승인 대기**를 거칩니다. 한 요청은 한 번만 실행됩니다.
- 앱 데이터(설정, 승인 대기열, 알림 기록)는 `%APPDATA%\com.sopo9880.canvasassistant`에 있습니다.

## 구조

| 경로 | 내용 |
|---|---|
| `src/` | React UI (대화, Canvas, 승인 대기, 자동화, 설정) |
| `src-tauri/src/codex.rs` | `codex app-server` JSON-RPC 브리지 (허용 목록, 읽기 전용 강제, Codex 실행 파일 탐색) |
| `src-tauri/src/canvas.rs` | Canvas REST 클라이언트 (페이지네이션, 요청 한도 재시도, 토큰 출처 제한, 쓰기 작업) |
| `src-tauri/src/mcp.rs` | Codex가 실행하는 Canvas MCP 서버 (`canvas-assistant.exe --mcp-canvas`) |
| `src-tauri/src/db.rs` | SQLite: 승인 대기열, 예약 작업, 알림 중복 방지, 활동 기록 |
| `src-tauri/src/scheduler.rs` | 마감·공지·성적 알림과 AI 브리핑 |
| `src-tauri/src/config.rs` | 설정 파일과 자격 증명 관리자 토큰 |
| `preview/` | 브라우저에서 UI를 확인하는 개발용 미리보기 (가짜 데이터) |

## 개발

```bash
npm ci
npm run fetch:codex           # 설치 파일에 넣을 codex.exe 받기 (package.json codexVersion)
npm test                      # 저장소 규칙 테스트
cargo test --manifest-path src-tauri/Cargo.toml   # Rust 단위·통합 테스트
npm run tauri dev             # 앱 실행 (Windows)
npm run ui:preview            # 브라우저 UI 미리보기: http://127.0.0.1:5199/?view=canvas
```

Windows에서 개발하려면 Rust stable, Node 22+, Visual Studio C++ Build Tools, WebView2가 필요합니다.

## 빌드와 릴리스

- **Windows에서**: `npx tauri build --bundles nsis`
- **Linux에서 크로스 빌드**: `npm run build:win-cross` → `release/`에 설치 파일과 `SHA256SUMS.txt`가 생깁니다 (MinGW, NSIS 필요. 자세한 내용은 스크립트 주석 참고).
- **GitHub Actions**: *Release* 워크플로를 수동 실행하면 Windows에서 빌드해 릴리스를 만듭니다. 저장소가 비공개인 동안은 Actions 사용량을 아끼려고 수동 실행만 켜 두었습니다.

버전은 `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` 세 곳을 함께 올립니다 (`npm test`가 일치 여부를 확인합니다).

## 알려진 제한

- 비공개 저장소에서는 자동 업데이트를 쓸 수 없어서, **설정 → 업데이트 확인**은 릴리스 페이지를 안내합니다. 저장소를 공개로 바꾸면 자동 확인이 동작합니다.
- 알림과 자동화는 앱(또는 트레이)이 실행 중일 때만 동작합니다.
- 파일 업로드 제출은 아직 지원하지 않습니다 (텍스트·URL 제출만 가능).

Canvas는 Instructure의 상표이며, 이 프로젝트는 Instructure나 OpenAI와 관계없는 개인 프로젝트입니다.
