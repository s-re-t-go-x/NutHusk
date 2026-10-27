import { el, toast } from './util.js';
import { showDialog } from './dialog.js';
import { listAvailableActions } from './state.js';

/**
 * 顺序执行流水线中的各个插件动作，前一步的输出作为下一步的输入。
 * 返回 { output, trace: [{ pluginName, actionName, output, ms }] }
 */
export async function executeWorkflowSteps(wf, rawInput, { plugins, executeAction }) {
  let currentText = String(rawInput ?? '');
  const trace = [];
  const byId = new Map(plugins.map((p) => [p.id, p]));

  for (let i = 0; i < wf.steps.length; i++) {
    const step = wf.steps[i];
    const p = byId.get(step.pluginId);
    if (!p || !p.enabled || p.error) {
      throw new Error(`步骤 ${i + 1}：插件「${p?.name ?? step.pluginId}」不可用或已禁用`);
    }
    const act = (p.actions ?? []).find((a) => a.id === step.actionId);
    if (!act) {
      throw new Error(`步骤 ${i + 1}：插件「${p.name}」未声明动作 ${step.actionId}`);
    }
    const t0 = performance.now();
    const res = await executeAction(p, act, currentText);
    const ms = Math.max(0, Math.round(performance.now() - t0));
    currentText = typeof res === 'string' ? res : JSON.stringify(res, null, 2);
    trace.push({
      pluginName: p.name,
      actionName: act.name || act.id,
      output: currentText,
      ms,
    });
  }

  return { output: currentText, trace };
}

/** 弹出流水线执行器对话框。若传入 initialInput，打开后自动执行一次。 */
export async function openWorkflowRunner(wf, initialInput = '', { plugins, executeAction, onSendTo }) {
  const inputArea = el('textarea', {
    class: 'src-input mono wf-textarea',
    rows: 5,
    placeholder: '在此粘贴或输入要处理的文本（按 Ctrl+Enter 立即执行）…',
    value: initialInput || '',
    spellcheck: false,
  });
  const outputArea = el('textarea', {
    class: 'src-input mono wf-textarea',
    rows: 6,
    readonly: true,
    placeholder: '流水线执行结果将显示在这里…',
    spellcheck: false,
  });
  const statusLine = el('div', {
    class: 'muted small',
    textContent: `共 ${wf.steps.length} 个步骤：${wf.description || wf.steps.map((s) => s.actionId).join(' → ')}`,
  });

  const runBtn = el('button', {
    class: 'btn primary',
    type: 'button',
    textContent: '⚡ 执行流水线',
  });
  const copyBtn = el('button', {
    class: 'btn',
    type: 'button',
    textContent: '复制结果',
    onclick: () => {
      if (!outputArea.value) return toast('还没有可复制的结果');
      outputArea.focus();
      outputArea.select();
      try { document.execCommand('copy'); } catch { /* 忽略 */ }
      outputArea.setSelectionRange(0, 0);
      toast('已复制流水线输出');
    },
  });
  const sendBtn = el('button', {
    class: 'btn',
    type: 'button',
    textContent: '投递到工具…',
    onclick: () => {
      const val = outputArea.value;
      if (!val.trim()) return toast('还没有可投递的结果');
      onSendTo?.(val, wf.name);
    },
  });

  async function doRun() {
    const src = inputArea.value;
    if (!src.trim()) {
      statusLine.textContent = '请先输入要处理的文本';
      return;
    }
    runBtn.disabled = true;
    statusLine.textContent = '正在执行流水线…';
    try {
      const { output, trace } = await executeWorkflowSteps(wf, src, { plugins, executeAction });
      outputArea.value = output;
      const summary = trace.map((t, idx) => `${idx + 1}. ${t.actionName} (${t.ms}ms)`).join(' → ');
      statusLine.textContent = `✓ 执行完成：${summary}`;
    } catch (err) {
      statusLine.textContent = `✗ 执行出错：${err?.message || err}`;
    } finally {
      runBtn.disabled = false;
    }
  }

  runBtn.addEventListener('click', doRun);
  inputArea.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      doRun();
    }
  });

  const body = el('div', { class: 'wf-runner' },
    statusLine,
    el('div', { class: 'wf-field' },
      el('div', { class: 'wf-label', textContent: '输入内容' }),
      inputArea),
    el('div', { class: 'wf-actions-bar' }, runBtn, copyBtn, sendBtn),
    el('div', { class: 'wf-field' },
      el('div', { class: 'wf-label', textContent: '最终输出' }),
      outputArea));

  if (initialInput && String(initialInput).trim()) {
    setTimeout(doRun, 10);
  } else {
    setTimeout(() => inputArea.focus(), 20);
  }

  await showDialog({
    title: `流水线：${wf.name}`,
    body,
    buttons: [{ label: '关闭', value: false, primary: true }],
  });
}

/** 弹出新建/编辑流水线对话框。 */
export async function openWorkflowBuilder(existingWf = null, { plugins, onSave }) {
  const available = listAvailableActions(plugins);
  if (available.length === 0) {
    toast('当前没有声明了可用动作（actions）的插件');
    return;
  }

  const hasCustomName = existingWf && existingWf.name && existingWf.name !== existingWf.description;
  const nameInput = el('input', {
    class: 'src-input',
    type: 'text',
    placeholder: '流水线名称（留空则自动按步骤名称生成）',
    value: hasCustomName ? existingWf.name : '',
  });

  const selectedSteps = existingWf?.steps?.length
    ? existingWf.steps.map((s) => ({ ...s }))
    : [
        { pluginId: available[0].plugin.id, actionId: available[0].action.id },
        available[1]
          ? { pluginId: available[1].plugin.id, actionId: available[1].action.id }
          : { pluginId: available[0].plugin.id, actionId: available[0].action.id },
      ];

  const stepsBox = el('div', { class: 'wf-steps-box' });

  function renderSteps() {
    stepsBox.replaceChildren(
      ...selectedSteps.map((step, idx) => {
        const curKey = `${step.pluginId}:${step.actionId}`;
        const select = el('select', {
          class: 'src-input wf-step-select',
          onchange: (e) => {
            const [pid, aid] = e.target.value.split(':');
            selectedSteps[idx] = { pluginId: pid, actionId: aid };
          },
        }, ...available.map((item) => el('option', {
          value: item.key,
          selected: item.key === curKey,
          textContent: item.label,
        })));
        select.value = curKey;

        return el('div', { class: 'wf-step-row' },
          el('span', { class: 'tag', textContent: `步骤 ${idx + 1}` }),
          select,
          el('button', {
            class: 'btn sm',
            type: 'button',
            disabled: idx === 0,
            title: '上移步骤',
            textContent: '↑',
            onclick: () => {
              if (idx <= 0) return;
              [selectedSteps[idx - 1], selectedSteps[idx]] = [selectedSteps[idx], selectedSteps[idx - 1]];
              renderSteps();
            },
          }),
          el('button', {
            class: 'btn sm',
            type: 'button',
            disabled: selectedSteps.length <= 1,
            title: '移除步骤',
            textContent: '×',
            onclick: () => {
              if (selectedSteps.length <= 1) return;
              selectedSteps.splice(idx, 1);
              renderSteps();
            },
          }));
      })
    );
  }

  renderSteps();

  const addStepBtn = el('button', {
    class: 'btn sm',
    type: 'button',
    textContent: '+ 添加步骤',
    onclick: () => {
      const def = available[0];
      selectedSteps.push({ pluginId: def.plugin.id, actionId: def.action.id });
      renderSteps();
    },
  });

  const body = el('div', { class: 'wf-builder' },
    el('p', { class: 'muted small', textContent: '将多个插件动作按顺序串联：上一步的输出将自动作为下一步的输入。' }),
    nameInput,
    stepsBox,
    el('div', { style: 'margin-top: 8px;' }, addStepBtn));

  const ok = await showDialog({
    title: existingWf ? '编辑快捷流水线' : '新建快捷流水线',
    body,
    buttons: [
      { label: '取消', value: false },
      { label: '保存流水线', value: true, primary: true },
    ],
  });

  if (!ok) return;

  const stepLabels = selectedSteps.map((s) => {
    const found = available.find((a) => a.plugin.id === s.pluginId && a.action.id === s.actionId);
    return found ? (found.action.name || found.action.id) : s.actionId;
  });
  const finalName = nameInput.value.trim() || stepLabels.join(' → ');
  const desc = stepLabels.join(' → ');

  onSave({
    id: existingWf?.id ?? `wf-${Date.now()}`,
    name: finalName,
    description: desc,
    steps: selectedSteps,
  });
}

/** 弹出保存当前场景工作区对话框。 */
export async function promptSaveWorkspace({ toolCount, hasSplit, onSave }) {
  const nameInput = el('input', {
    class: 'src-input',
    type: 'text',
    placeholder: '工作区名称（如：前端调试、接口联调、日志排查）',
  });
  const summary = `将保存当前打开的 ${toolCount} 个工具标签${hasSplit ? '（含左右双栏分屏布局）' : ''}，随时一键切换恢复。`;
  const body = el('div', { class: 'wf-builder' },
    el('p', { class: 'muted small', textContent: summary }),
    nameInput);

  setTimeout(() => nameInput.focus(), 20);

  const ok = await showDialog({
    title: '保存场景工作区',
    body,
    buttons: [
      { label: '取消', value: false },
      { label: '保存工作区', value: true, primary: true },
    ],
  });
  if (!ok) return;
  const name = nameInput.value.trim();
  if (!name) return toast('请输入工作区名称');
  onSave(name);
}
