import { useState } from "react";

import garden from "../assets/loom-blue-garden.png";

export function StudioWelcome() {
  const [paused, setPaused] = useState(false);
  return <div className={`studio-welcome${paused ? " is-paused" : ""}`}>
    <div className="studio-art" aria-hidden="true">
      <img src={garden} alt="" fetchPriority="high" />
    </div>
    <div className="studio-art-top"><span>HUMAN KNOW-HOW × AI</span><span>LOOM / 01</span></div>
    <div className="studio-caption"><span>COLLECT. SHARE. CREATE.</span><h1>Good ideas,<br /><em>well woven.</em></h1><p>让好方法，成为共同习惯。</p></div>
    <div className="studio-art-bottom"><span>从一个人的经验，到整个团队的日常。</span><button type="button" aria-label={paused ? "播放背景动效" : "暂停背景动效"} onClick={() => setPaused(!paused)}>{paused ? "▶" : "Ⅱ"}</button></div>
  </div>;
}
