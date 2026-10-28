# Canvas API — 학생 토큰으로 할 수 있는 것

학생 계정의 액세스 토큰 기준입니다. 학교마다 기능(New Quizzes, Smart Search 등)과 권한 설정이 달라서 실제 동작은 학교 Canvas에서 확인해야 합니다.
상태: ✅ 구현 (v0.2.0 기준) · 🟡 일부 · ⬜ 미구현 · 🚫 구현 안 함

## 1. 조회

| 영역 | 엔드포인트 | 상태 | 비고 |
|---|---|---|---|
| 프로필 | `GET /users/self/profile` | ✅ | |
| 강의 목록·점수 | `GET /courses?include[]=total_scores&include[]=term` | ✅ | |
| 할 일 / 다가오는 일정 | `GET /users/self/todo`, `/users/self/upcoming_events` | ✅ | |
| 플래너 항목 | `GET /planner/items?start_date&end_date&filter=new_activity` | ✅ | |
| 과제 목록·상세 | `GET /courses/:id/assignments?include[]=submission`, `/assignments/:id` | ✅ | |
| 과제 루브릭 | 과제 상세의 `rubric`, `rubric_settings` | ✅ | 채점 기준을 AI 분석에 쓰기 좋음 |
| 과제 그룹·가중치 | `GET /courses/:id/assignment_groups?include[]=assignments` | ✅ | |
| 내 제출물 | `GET /courses/:id/students/submissions`, `GET .../submissions/:user_id?include[]=submission_comments,rubric_assessment,submission_history` | ✅ | |
| 공지 | `GET /announcements?context_codes[]` | ✅ | |
| 토론 | `GET /courses/:id/discussion_topics`, `/discussion_topics/:id/view` | ✅ | |
| 모듈·페이지 | `GET /courses/:id/modules?include[]=items`, `/pages/:url` | ✅ | |
| 파일·폴더 | `GET /courses/:id/files`, `/folders`, `GET /files/:id` | ✅ | 다운로드 포함 |
| 파일 내용 | `GET /files/:id` → `url` 내려받기 → 텍스트 추출 | ✅ | PDF, docx, pptx, xlsx, hwp, hwpx, 코드, zip, ipynb |
| 내 제출물 내용 | `GET .../submissions/self?include[]=submission_history` → 첨부 `url` | ✅ | 차수(attempt)별 |
| 캘린더 | `GET /calendar_events?context_codes[]&type=event|assignment` | ✅ | |
| 성적 | `GET /users/self/enrollments` (`grades`) | ✅ | |
| 메시지함 | `GET /conversations`, `/conversations/:id` | ✅ | 교수·조교 메시지 |
| 퀴즈(Classic) | `GET /courses/:id/quizzes`, `/quizzes/:id` | ✅ | New Quizzes는 학생용 API가 거의 없음 |
| 피어 리뷰 | `GET /courses/:id/assignments/:id/peer_reviews` | ✅ | 내가 해야 할 리뷰 |
| 그룹 | `GET /users/self/groups`, `/groups/:id/users` | ✅ | 조별 과제 |
| 강의 활동 | `GET /courses/:id/activity_stream`, `/users/self/activity_stream/summary` | ✅ | 새 소식 개수 |
| 학습 성과 | `GET /courses/:id/outcome_results` | 🟡 (canvas_api_get) | 학교에서 쓸 때만 |

## 1-1. LearningX 강의자료실 (LTI)

일부 학교(동아대 등)는 '강의자료실' 메뉴가 Canvas 파일이 아니라 LearningX(Coursemos) 외부 도구입니다.

1. `GET /courses/:id/tabs` → `context_external_tool_N` 탭 (라벨에 '강의자료' 우선)
2. `GET /courses/:id/external_tools/sessionless_launch?id=N&launch_type=course_navigation` → 일회용 `url` (토큰 사용)
3. 그 `url`을 **토큰 없이** 열어 `tool_form` LTI 양식을 읽고, 양식 `action`(예: `/learningx/lti/courseresource`)으로 POST
4. 응답 쿠키의 `xn_api_token`(없으면 페이지 스크립트에서 찾음)을 `Authorization: Bearer`로 `GET /learningx/api/v1/courses/:id/resources`
5. 내려받기: `https://cmaker.<학교 도메인>/index.php?module=xn_media_content2013&act=dispXn_media_content2013DownloadContent&content_id=ID` (자료 목록의 URL에서 호스트를 찾음)

쿠키는 메모리에만 두고 20분 뒤 다시 실행합니다. 동영상 같은 웹 콘텐츠는 파일로 받을 수 없어 브라우저로 엽니다.

## 2. 변경 (학생 권한)

| 작업 | 엔드포인트 | 상태 | 비고 |
|---|---|---|---|
| **과제 제출 — 텍스트** | `POST /courses/:id/assignments/:aid/submissions` `submission[submission_type]=online_text_entry`, `submission[body]` | ✅ (승인 대기열) | |
| **과제 제출 — URL** | 같은 경로, `online_url`, `submission[url]` | ✅ (승인 대기열) | |
| **과제 제출 — 파일** | ① `POST .../submissions/self/files` (name, size, content_type) → ② `upload_url`에 multipart 업로드(**토큰 없이**, `upload_params` 그대로, `file`은 마지막) → ③ `Location`을 토큰과 함께 GET → ④ `online_upload` + `submission[file_ids][]` | ✅ | 500MB 이하 |
| 제출 + 코멘트 | 제출 요청에 `comment[text_comment]` | ✅ | |
| 제출물에 코멘트 달기 | `PUT /courses/:id/assignments/:aid/submissions/self` `comment[text_comment]`, 첨부는 `.../submissions/comments/self/files`로 업로드 | ✅ | |
| 미디어 제출 | `media_recording` + `media_comment_id` | ⬜ | 미디어 업로드 절차가 복잡해 후순위 |
| Classic 퀴즈 응시 | `POST /courses/:id/quizzes/:qid/submissions` → `GET /quiz_submissions/:id/questions` → `POST /quiz_submissions/:id/questions` (attempt, validation_token, 답) → `POST .../submissions/:id/complete` | 🚫 | AI 대리 응시는 학업 부정이라 구현하지 않음. 응시는 Canvas에서 |
| 토론 글·답글 | `POST /courses/:id/discussion_topics/:tid/entries`, `.../entries/:eid/replies` | ✅ | |
| 토론 읽음·좋아요 | `PUT .../discussion_topics/:tid/read_all`, `.../entries/:eid/rating` | 🟡 읽음만 | |
| 공지 읽음 처리 | 토론 읽음과 같은 엔드포인트 | ✅ | |
| 모듈 항목 완료 | `PUT /courses/:id/modules/:mid/items/:iid/done`, `POST .../mark_read` | ✅ | |
| 플래너 메모 | `POST /planner_notes`, `PUT/DELETE /planner_notes/:id` | 🟡 | 추가만 구현 |
| 플래너 완료·숨김 | `POST /planner/overrides` (`plannable_type`: assignment, quiz, discussion_topic, wiki_page, announcement, planner_note / `marked_complete`, `dismissed`) | ✅ | 우클릭 메뉴 |
| 개인 캘린더 일정 | `POST /calendar_events` (`context_code=user_:id`) | ✅ (승인 대기열) | |
| 메시지 보내기 | `POST /conversations` (recipients[], body, subject) | ✅ | 앱에서 직접(확인 창) 또는 AI 제안(승인) |
| 내 파일 업로드 | `POST /users/self/files` (위 3단계) | ⬜ | |
| What-if 점수 | `PUT /submissions/:id/what_if_grades` (`student_entered_score`), `PUT /courses/:id/what_if_grades/reset` | ✅ | 학교 설정에 따라 막혀 있을 수 있음 |
| 알림 설정 | `PUT /users/self/communication_channels/:id/notification_preferences` | ⬜ | 필요성 낮음 |

## 3. 안 되는 것 / 주의

- **New Quizzes** 응시·답안: 학생용 공개 API가 사실상 없음.
- 교수자용 기능(채점, 출석, 성적부 편집)은 학생 토큰으로 불가.
- 외부 도구(LTI)로 내는 과제(Turnitin 등 일부)는 API 제출 대신 Canvas 웹에서 내야 할 수 있음.
- 제출 횟수 제한·마감 잠금(`lock_at`)이 있으면 API도 거부. 제출 전에 `allowed_attempts`, `locked_for_user`, `submission_types`를 확인해야 함.
- 파일 업로드 2단계에는 Authorization 헤더를 보내면 안 됨 (저장소 서버로 토큰 유출).
- 요청 한도: `X-Rate-Limit-Remaining`이 바닥나면 403. 이미 재시도 처리 있음.
- 일부 학교는 GraphQL(`POST /api/graphql`)만 지원하는 필드가 있음 — 필요할 때만.

## 출처
- Submissions API (canvas.wisc.edu/doc/api/submissions.html)
- File Uploads (developerdocs.instructure.com/services/canvas/basics/file.file_uploads)
- Quiz Submission Questions (developerdocs.instructure.com/services/canvas/resources/quiz_submission_questions)
- What If Grades (developerdocs.instructure.com/services/canvas/file.all_resources/what_if_grades)
- Planner Override (learning.flyfreely.io/doc/api/planner_override.html)
- 전체 리소스 목록 (canvas.wisc.edu/doc/api/index.html)
