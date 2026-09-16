import { useState } from "react";
import garden from "../assets/loom-blue-garden.webp";

export function StudioWelcome({ blue }: { blue: boolean }) {
  const [paused, setPaused] = useState(false);
  if (blue) return <div className={`studio-welcome${paused ? " is-paused" : ""}`}>
    <div className="studio-art" aria-hidden="true">
      <img src={garden} alt="" fetchPriority="high" />
    </div>
    <div className="studio-art-top"><span>HUMAN KNOW-HOW × AI</span><span>LOOM / 01</span></div>
    <div className="studio-caption"><span>COLLECT. SHARE. CREATE.</span><h1>Good ideas,<br /><em>well woven.</em></h1><p>让好方法，成为共同习惯。</p></div>
    <div className="studio-art-bottom"><span>从一个人的经验，到整个团队的日常。</span><button type="button" aria-label={paused ? "播放背景动效" : "暂停背景动效"} onClick={() => setPaused(!paused)}>{paused ? "▶" : "Ⅱ"}</button></div>
  </div>;

  return (
    <div className="studio-welcome">
      <span className="eyebrow">YOUR TEAM. YOUR KNOW-HOW.</span>
      <h1>好方法，<br />值得被<span>反复使用。</span></h1>
      <p className="welcome-description">把一个人的经验，变成整个团队的技能。<br />收集、分享，在你习惯的 AI 工具里继续创造。</p>
      <div className="method-preview" aria-hidden="true">
        <div className="method-file"><span>▤</span> SKILL.md <span className="method-file-tag">YOUR NEXT IDEA</span></div>
        <div className="method-code"><span>01</span><strong># 把经验写成方法</strong><span>02</span><i>让每一次实践，都成为下一次的起点。</i><span>03</span><i /><span>04</span><b>分享给团队 → 安装到工具 → 一起改进</b></div>
        <div className="method-footer"><span className="method-light" />一个文件，无限次好用。</div>
      </div>
      <div className="welcome-footnote">BUILT BY PEOPLE. REUSED WITH AI.</div>
    </div>
  );
}
