import { useEffect, useId, useRef, useState } from "react";
import type { FormEvent } from "react";
import chatgptLogo from "../assets/chatgpt-logo.svg";
import type { Skill } from "./client";
import "./ask-chat.css";

export type AskSkillHit = Pick<Skill, "id" | "slug" | "title" | "description">;

export type AskAnswer = {
  text: string;
  skills?: AskSkillHit[];
};

type AskTurn = {
  id: number;
  role: "user" | "assistant";
} & AskAnswer;

const GENERIC_QUERY = /^(技能|skill|skills|找技能|有什么|帮我|你好|hi|hello)$/i;

export function answerAsk(
  question: string,
  skills: AskSkillHit[],
  signedIn: boolean,
): AskAnswer {
  const q = question.trim();
  if (!q) throw new Error("问题不能为空。");

  if (/登录|验证码|邮箱|注册/.test(q)) {
    return {
      text: "用工作邮箱收取验证码即可登录。首次验证会创建账号，不用设密码。网页刷新后要重新验证；桌面 App 会记住这次登录。",
    };
  }
  if (/分享|发布|上传/.test(q)) {
    return {
      text: "打开团队技能页，点「分享技能」。写清用途和一个能跑通的示例，同事就能在自己的工具里用。",
    };
  }
  if (/本机|安装|导入|激活/.test(q)) {
    return {
      text: "本机技能只在桌面 App 里管理。选好团队技能后，可以预览安装并激活到当前项目的 agent 目录。浏览器里只能看说明，不能改本机文件。",
    };
  }
  if (/更新|版本/.test(q)) {
    return {
      text: "「版本更新」会对照团队目录和本机已装版本。先检查，再预览，确认后才写入本机仓库。",
    };
  }
  if (/主题|深色|蓝白|外观/.test(q)) {
    return {
      text: "右下角可以在蓝白版和深色版之间切换。选择保存在这台电脑上。",
    };
  }
  if (/你是谁|能做什么|怎么用|帮助/.test(q)) {
    return {
      text: "我是 Ask。可以在当前团队目录里找技能，也可以说明登录、分享、本机安装和版本更新。试着说出技能名称。",
    };
  }
  if (/团队|工作空间/.test(q)) {
    return {
      text: signedIn
        ? "顶部可以切换当前工作空间。每个团队有自己的技能目录、成员和发布。"
        : "登录后选择或创建一个团队，技能都按工作空间分开。",
    };
  }

  if (!signedIn) {
    return {
      text: "登录后我就能在团队目录里帮你找技能。也可以先问「怎么登录」。",
    };
  }

  if (GENERIC_QUERY.test(q)) {
    if (!skills.length) {
      return {
        text: "这个团队还没有技能。你可以从「分享技能」放进第一份方法。",
      };
    }
    return {
      text: `当前目录里有 ${skills.length} 个技能。点下面一条打开，或直接说出名称。`,
      skills: skills.slice(0, 5),
    };
  }

  const needle = q.toLowerCase();
  const hits = skills.filter((skill) =>
    [skill.title, skill.slug, skill.description].some((field) =>
      field.toLowerCase().includes(needle),
    ),
  );
  if (hits.length) {
    return {
      text:
        hits.length === 1
          ? "找到这个技能，点它可以打开详情。"
          : `找到 ${hits.length} 个相关技能。`,
      skills: hits.slice(0, 6),
    };
  }

  if (!skills.length) {
    return {
      text: "这个团队还没有技能，所以我没法按名称查找。先分享一个，或问「怎么登录」。",
    };
  }
  return {
    text: "目录里没有名称或用途对得上的技能。换个关键词，或问「有什么」。",
  };
}

function ChatGPTLogo({ size = 40 }: { size?: number }) {
  return (
    <img
      src={chatgptLogo}
      alt=""
      width={size}
      height={size}
      draggable={false}
    />
  );
}

interface AskChatProps {
  skills: AskSkillHit[];
  signedIn: boolean;
  onOpenSkill: (skill: AskSkillHit) => void;
}

const GREETING: AskAnswer = {
  text: "想找哪个技能，或问我怎么登录、分享、装到本机。",
};

export function AskChat({ skills, signedIn, onOpenSkill }: AskChatProps) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const [turns, setTurns] = useState<AskTurn[]>([
    { id: 0, role: "assistant", ...GREETING },
  ]);
  const nextId = useRef(1);
  const dockRef = useRef<HTMLDivElement>(null);
  const pillRef = useRef<HTMLButtonElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const logRef = useRef<HTMLDivElement>(null);
  const wasOpen = useRef(false);
  const sheetId = useId();
  const titleId = useId();

  useEffect(() => {
    if (!open) {
      if (wasOpen.current) pillRef.current?.focus();
      wasOpen.current = false;
      return;
    }
    wasOpen.current = true;
    inputRef.current?.focus();
    const log = logRef.current;
    if (log && typeof log.scrollTo === "function")
      log.scrollTo({ top: log.scrollHeight });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        setOpen(false);
      }
    };
    const onPointer = (event: PointerEvent) => {
      if (!dockRef.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    document.addEventListener("pointerdown", onPointer);
    return () => {
      window.removeEventListener("keydown", onKey);
      document.removeEventListener("pointerdown", onPointer);
    };
  }, [open]);

  const suggestions = signedIn
    ? ["有什么", "如何分享技能", "本机怎么用"]
    : ["怎么登录", "你能做什么", "本机怎么用"];

  const send = (text: string) => {
    const question = text.trim();
    if (!question) return;
    const userId = nextId.current++;
    setTurns((current) => [
      ...current,
      { id: userId, role: "user", text: question },
    ]);
    setDraft("");
    try {
      const answer = answerAsk(question, skills, signedIn);
      const assistantId = nextId.current++;
      setTurns((current) => [
        ...current,
        { id: assistantId, role: "assistant", ...answer },
      ]);
    } catch (err) {
      const assistantId = nextId.current++;
      setTurns((current) => [
        ...current,
        {
          id: assistantId,
          role: "assistant",
          text: `没能回答：${err instanceof Error ? err.message : String(err)}`,
        },
      ]);
    }
    queueMicrotask(() => {
      const log = logRef.current;
      if (log && typeof log.scrollTo === "function")
        log.scrollTo({ top: log.scrollHeight });
    });
  };

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    send(draft);
  };

  return (
    <div
      className="ask-dock"
      ref={dockRef}
      data-open={open}
      data-conversation={turns.length > 1}
    >
      {open && (
        <div
          className="ask-sheet"
          id={sheetId}
          role="dialog"
          aria-modal="false"
          aria-labelledby={titleId}
        >
          <div className="ask-sheet-head">
            <span className="ask-logo">
              <ChatGPTLogo size={40} />
            </span>
            <div className="ask-heading">
              <h2 id={titleId}>Ask</h2>
              <p>Loom 团队助手</p>
            </div>
            <button
              className="ask-close"
              type="button"
              aria-label="关闭 Ask"
              onClick={() => setOpen(false)}
            >
              ×
            </button>
          </div>
          <div
            className="ask-log"
            ref={logRef}
            role="log"
            aria-label="Ask 对话"
            aria-live="polite"
          >
            {turns.map((turn) => (
              <div key={turn.id} className={`ask-turn ask-turn-${turn.role}`}>
                {turn.role === "assistant" && (
                  <span className="ask-avatar" aria-hidden="true">
                    <ChatGPTLogo size={24} />
                  </span>
                )}
                <div className="ask-turn-content">
                  <div className={`ask-bubble ask-bubble-${turn.role}`}>
                    <p>{turn.text}</p>
                  </div>
                  {turn.skills && turn.skills.length > 0 && (
                    <div className="ask-hits">
                      {turn.skills.map((skill) => (
                        <button
                          key={skill.id}
                          className="ask-hit"
                          type="button"
                          onClick={() => onOpenSkill(skill)}
                        >
                          <span className="ask-hit-icon" aria-hidden="true">
                            〈/〉
                          </span>
                          <span className="ask-hit-copy">
                            <strong>{skill.title || skill.slug}</strong>
                            <span className="ask-hit-description">
                              {skill.description || skill.slug}
                            </span>
                            <span className="ask-hit-tag">团队技能</span>
                          </span>
                          <span className="ask-hit-arrow" aria-hidden="true">
                            →
                          </span>
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              </div>
            ))}
          </div>
          <div className="ask-followups">
            <p>你可能还想问</p>
            <div className="ask-suggestions">
              {suggestions.map((item) => (
                <button
                  key={item}
                  className="ask-chip"
                  type="button"
                  onClick={() => send(item)}
                >
                  {item}
                  <span aria-hidden="true">›</span>
                </button>
              ))}
            </div>
          </div>
          <form className="ask-composer" onSubmit={onSubmit}>
            <input
              ref={inputRef}
              aria-label="问 Ask"
              placeholder="继续问 Ask…"
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
            />
            <button
              className="ask-send"
              type="submit"
              aria-label="发送"
              disabled={!draft.trim()}
            >
              ↑
            </button>
          </form>
        </div>
      )}
      <button
        ref={pillRef}
        className="ask-pill"
        type="button"
        aria-label="Ask"
        aria-expanded={open}
        aria-controls={open ? sheetId : undefined}
        onClick={() => setOpen((current) => !current)}
      >
        <span className="ask-pill-fill" aria-hidden="true" />
        <span className="ask-pill-spec" aria-hidden="true" />
        <span className="ask-pill-edge" aria-hidden="true" />
        <span className="ask-logo">
          <ChatGPTLogo />
        </span>
        <span className="ask-pill-label">
          <span className="ask-a">A</span>
          <span className="ask-s">s</span>k
        </span>
      </button>
    </div>
  );
}
