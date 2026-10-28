import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./styles.css";

type Model = { id: string; model?: string; displayName?: string; defaultReasoningEffort?: string; supportedReasoningEfforts?: Array<{reasoningEffort:string}> };
type Message = {role:"user"|"assistant"; text:string};
type Thread = {id:string; name?:string; updatedAt?:number};
const call = (method:string, params:Record<string,unknown>={}) => invoke<any>("codex_call",{method,params});

function App(){
 const [tab,setTab]=useState<"chat"|"settings"|"canvas">("chat");
 const [ready,setReady]=useState(false),[error,setError]=useState("");
 const [account,setAccount]=useState("확인 중"),[code,setCode]=useState(""),[url,setUrl]=useState("");
 const [models,setModels]=useState<Model[]>([]),[selected,setSelected]=useState(""),[effort,setEffort]=useState("medium");
 const [threads,setThreads]=useState<Thread[]>([]),[thread,setThread]=useState(""),[turn,setTurn]=useState("");
 const [messages,setMessages]=useState<Message[]>([]),[draft,setDraft]=useState(""),[busy,setBusy]=useState(false);
 const [historyError,setHistoryError]=useState("");
 async function loadThreads(){try{const result=await call("thread/list");setThreads(result.data??result.threads??[]);}catch(e){setHistoryError(String(e));}}
 async function start(){
  try{setError("");await invoke("codex_start");setReady(true);
   const a=await call("account/read");setAccount(a.account?.email??(a.account?"로그인됨":"로그인 필요"));
   const m=await call("model/list");const items:Model[]=m.data??m.models??[];setModels(items);
   if(items.length){const first=items.find(x=>x.isDefault)??items[0];setSelected(first.model??first.id);setEffort(first.defaultReasoningEffort??"medium")}
   await loadThreads();
  }catch(e){setError(String(e));setReady(false)}
 }
 useEffect(()=>{void start();const unlisten=listen<{method:string;params:any}>("codex-event",evt=>{
  const {method,params}=evt.payload;
  if(method==="item/agentMessage/delta"&&typeof params?.delta==="string"){
   setMessages(old=>{const next=[...old];if(next.at(-1)?.role==="assistant")next[next.length-1]={role:"assistant",text:next.at(-1)!.text+params.delta};else next.push({role:"assistant",text:params.delta});return next;});
  }
  if(method==="turn/started")setTurn(params?.turn?.id??"");
  if(method==="turn/completed"||method==="turn/failed"){setBusy(false);setTurn("");void loadThreads();}
  if(method==="account/login/completed")void call("account/read").then(x=>setAccount(x.account?.email??"로그인됨")).catch(()=>{});
 });return()=>{void unlisten.then(f=>f())}},[]);
 async function login(){
  try{setError("");const r=await call("account/login/start");setCode(r.userCode??"");setUrl(r.verificationUrl??"https://auth.openai.com/codex/device");}
  catch(e){setError(String(e))}
 }
 async function openLogin(){if(url.startsWith("https://auth.openai.com/"))await openUrl(url)}
 async function openThread(id:string){
  try{setError("");await call("thread/resume",{threadId:id});setThread(id);setMessages([]);const r=await call("thread/turns/list",{threadId:id});const turns:any[]=r.data??r.turns??[];const list:Message[]=[];
   for(const t of [...turns].reverse())for(const item of (t.items??[])){if(item.type==="userMessage")list.push({role:"user",text:(item.content??[]).map((c:any)=>c.text??"").join("\n")});if(item.type==="agentMessage")list.push({role:"assistant",text:item.text??""})}setMessages(list);
  }catch(e){setError(String(e))}
 }
 async function send(){
  const text=draft.trim();if(!text||busy||!ready)return;
  setBusy(true);setDraft("");setError("");setMessages(prev=>[...prev,{role:"user",text}]);
  try{let id=thread;if(!id){const r=await call("thread/start",{model:selected});id=r.thread?.id??r.id;if(!id)throw Error("Codex did not return a thread ID");setThread(id)}
   await call("turn/start",{threadId:id,text,model:selected,effort});
  }catch(e){setBusy(false);setError(String(e))}
 }
 async function interrupt(){if(thread&&turn)try{await call("turn/interrupt",{threadId:thread,turnId:turn});setBusy(false)}catch(e){setError(String(e))}}
 const model=models.find(m=>(m.model??m.id)===selected);
 return <div className="app">
  <aside><div className="brand"><img src="/logo.svg" alt="Canvas Assistant"/>Canvas Assistant</div>
   <button className="new" onClick={()=>{setTab("chat");setThread("");setMessages([]);}}>＋ 새 대화</button>
   <nav><button className={tab==="chat"?"active":""} onClick={()=>setTab("chat")}>☷ 대화</button><button className={tab==="canvas"?"active":""} onClick={()=>setTab("canvas")}>▤ Canvas</button><button className={tab==="settings"?"active":""} onClick={()=>setTab("settings")}>⚙ 설정</button></nav>
   <div className="subhead">최근 대화</div><div className="threads">{threads.map(t=><button key={t.id} className={t.id===thread?"active":""} onClick={()=>void openThread(t.id)}>{t.name??"대화 "+t.id.slice(0,8)}</button>)}</div>
   {historyError&&<small>{historyError}</small>}<div className="footer">{ready?"● Codex 연결됨":"○ Codex 연결 안 됨"}</div>
  </aside>
  <main><header><h2>{tab==="chat"?"AI 대화":tab==="settings"?"설정":"Canvas LMS"}</h2>{tab==="chat"&&<div className="model"><select aria-label="Codex 모델" value={selected} onChange={e=>{setSelected(e.target.value);setEffort(models.find(m=>(m.model??m.id)===e.target.value)?.defaultReasoningEffort??"medium")}}>{models.length===0?<option value="">모델을 불러오는 중</option>:models.map(m=><option key={m.id} value={m.model??m.id}>{m.displayName??m.model??m.id}</option>)}</select><select aria-label="추론 수준" value={effort} onChange={e=>setEffort(e.target.value)}>{(model?.supportedReasoningEfforts?.map(e=>e.reasoningEffort)??["low","medium","high","xhigh"]).map(v=><option key={v} value={v}>{v}</option>)}</select></div>}</header>
  {error&&<div role="alert" className="error">{error}<button onClick={()=>setError("")}>닫기</button></div>}
  {tab==="chat"?<><section className="conversation">{messages.length?messages.map((m,i)=><div key={i} className={"msg "+m.role}><b>{m.role==="assistant"?"Canvas Assistant":"나"}</b><div className="content">{m.text}</div></div>):<div className="welcome"><img src="/logo.svg" alt=""/><h1>무엇을 도와드릴까요?</h1><p>Codex와 대화하거나 Canvas 학습 정보를 관리하세요.</p></div>}</section><div className="composer"><textarea value={draft} onChange={e=>setDraft(e.target.value)} onKeyDown={e=>{if(e.key==="Enter"&&!e.shiftKey){e.preventDefault();void send()}}} placeholder="Codex에게 메시지 보내기..." disabled={!ready||busy}/>{busy?<button onClick={()=>void interrupt()}>■ 중단</button>:<button onClick={()=>void send()} disabled={!ready||!draft.trim()}>전송 ↑</button>}</div></>:
  tab==="settings"?<section className="panel"><h3>Codex 계정</h3><p>상태: {account}</p><button onClick={()=>void start()}>다시 연결</button> <button onClick={()=>void login()} disabled={!ready}>기기 코드 로그인</button>{code&&<div className="device"><strong>{code}</strong><button onClick={()=>void openLogin()}>공식 인증 페이지 열기</button><p>인증 완료 후 연결 상태를 다시 확인하세요.</p></div>}<h3>Canvas 연결</h3><p>Canvas API URL·토큰 저장은 2단계 개발 대상입니다. 현재 비밀정보를 입력하거나 저장하지 않습니다.</p><h3>자동화·업데이트</h3><p>Windows 자동 시작, 스케줄러, 트레이, 설치형 업데이트는 3단계 개발 대상입니다.</p></section>:
  <section className="panel"><h3>Canvas LMS 연동 준비 중</h3><p>강의·과제·공지·성적·파일 관리와 AI MCP 도구는 2단계에서 안전한 인증 및 API 접근 제어와 함께 제공됩니다.</p><button onClick={()=>setTab("settings")}>연결 설정 안내</button></section>}
  </main></div>
}
createRoot(document.getElementById("root")!).render(<React.StrictMode><App/></React.StrictMode>);
