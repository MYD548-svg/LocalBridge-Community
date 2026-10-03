import { useState } from "react";
import { bridge, type ProjectProjection, type UiError } from "../../bridge";
import { UiErrorNotice } from "../../components/UiErrorNotice";

export function projectNameFromPath(path: string): string {
  const name = path.replace(/[\\/]+$/, "").split(/[\\/]/).pop()?.trim();
  return name && !name.endsWith(":") ? name : "本地项目";
}

export function ProjectManager({ projects, error, onRun, onSelect, onRemove, onClose }: {
  projects: ProjectProjection[];
  error: UiError | null;
  onRun: (action: () => Promise<void>) => Promise<void>;
  onSelect: (id: string) => void;
  onRemove: (project: ProjectProjection) => void;
  onClose: () => void;
}) {
  const [draft, setDraft] = useState<{ id: string | null; path: string; name: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try { await onRun(action); } finally { setBusy(false); }
  };
  const add = () => void run(async () => {
    const path = await bridge.chooseProjectFolder();
    if (path) setDraft({ id: null, path, name: projectNameFromPath(path) });
  });
  const save = () => void run(async () => {
    if (!draft?.name.trim()) return;
    if (draft.id) {
      await bridge.renameProject(draft.id, draft.name.trim());
      setDraft(null);
    } else {
      const id = await bridge.addProjectDeferred(draft.path, draft.name.trim());
      setDraft(null);
      if (!projects.some((project) => project.active)) onSelect(id);
    }
  });
  return <div className="sheet-backdrop" onMouseDown={() => { if (!busy) onClose(); }}>
    <section className="sheet" role="dialog" aria-modal="true" aria-label="项目管理" onMouseDown={(event) => event.stopPropagation()}>
      <h2>项目管理</h2>
      {error && <UiErrorNotice error={error}/>}
      {!projects.length && <p>添加项目后即可使用。为文件夹起一个容易识别的名称。</p>}
      <div className="project-list">{projects.map((project) => <div className="project-item" key={project.id}>
        <div className="project-description"><strong className="project-name" title={project.name}>{project.name}</strong><span className="project-path">{project.path}</span></div>
        <div className="project-item-actions">
          {project.active ? <span className="project-current">当前</span> : <button className="secondary" disabled={busy} onClick={() => onSelect(project.id)}>使用</button>}
          <button className="secondary" disabled={busy} onClick={() => setDraft({ id: project.id, name: project.name, path: project.path })}>重命名</button>
          <button className="secondary" disabled={busy} onClick={() => onRemove(project)}>移除</button>
        </div>
      </div>)}</div>
      {draft && <form className="project-form" onSubmit={(event) => { event.preventDefault(); save(); }}>
        <h3>{draft.id ? "重命名项目" : "添加项目"}</h3>
        <div className="field"><label htmlFor="project-name">项目名称</label><input id="project-name" type="text" autoFocus value={draft.name} disabled={busy} onChange={(event) => setDraft({ ...draft, name: event.target.value })}/></div>
        <p className="project-path">{draft.path}</p>
        <div className="dialog-actions"><button type="button" className="secondary" disabled={busy} onClick={() => setDraft(null)}>取消</button><button type="submit" className="primary" disabled={busy || !draft.name.trim()}>{busy ? "保存中…" : "保存"}</button></div>
      </form>}
      <div className="dialog-actions"><button className="secondary" disabled={busy} onClick={add}>添加项目</button><button className="primary" disabled={busy} onClick={onClose}>完成</button></div>
    </section>
  </div>;
}
