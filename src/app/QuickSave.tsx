import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from 'react';
import { api } from '../api';
import { AppError, type QuickSavePrefill, type VaultStatus } from '../api/types';
import { Keys } from '../components/bits';
import { GeneratorPanel } from '../components/Generator';
import { Icon } from '../components/Icon';
import { StrengthMeter } from '../components/Strength';
import { usePrefs } from './contexts';

interface Form {
  title: string;
  username: string;
  password: string;
  url: string;
  groupId: string;
}

const EMPTY: Form = { title: '', username: '', password: '', url: '', groupId: '' };

/** How many entries already saved for the site are named. */
const EXISTING_SHOWN = 3;

interface Props {
  status: VaultStatus;
  /** Esc: close, focus back to the window behind */
  onClose: () => void;
  /** saved: close after `ms`, focus back to the window behind */
  onDone: (ms: number) => void;
}

/**
 * Quick save (features.md §3.12): the quick search box as a small form for a
 * new entry. Title and URL come from the window it was opened over (a
 * browser page gives its site); the generator makes the password.
 */
export function QuickSave({ status, onClose, onDone }: Props) {
  const { t, locale } = usePrefs();
  const [prefill, setPrefill] = useState<QuickSavePrefill | null>(null);
  const [form, setForm] = useState<Form>(EMPTY);
  const [showPassword, setShowPassword] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [picking, setPicking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ text: string; ok: boolean } | null>(null);
  const titleInput = useRef<HTMLInputElement>(null);
  const userInput = useRef<HTMLInputElement>(null);
  const passwordInput = useRef<HTMLInputElement>(null);
  const groupButton = useRef<HTMLButtonElement>(null);
  // what the user typed before the prefill arrived wins
  const typed = useRef(new Set<keyof Form>());

  useEffect(() => {
    let live = true;
    api.quickSavePrefill().then(
      (p) => {
        if (!live) return;
        setPrefill(p);
        setForm((f) => {
          const keep = (k: keyof Form, v: string) => (typed.current.has(k) ? f[k] : v);
          return {
            ...f,
            title: keep('title', p.title),
            url: keep('url', p.url),
            username: keep('username', p.username),
            groupId: p.groupId,
          };
        });
        // start where the typing is: the title when there is nothing to go
        // on, the password when the user name came along too
        if (typed.current.size === 0) {
          if (!p.title) titleInput.current?.focus();
          else if (p.username) passwordInput.current?.focus();
        }
      },
      (e: unknown) =>
        live && setNote({ text: t(`error.${e instanceof AppError ? e.code : 'internal'}`), ok: false }),
    );
    return () => {
      live = false;
    };
  }, [t]);

  const set = (key: keyof Form, value: string) => {
    typed.current.add(key);
    setForm((f) => ({ ...f, [key]: value }));
    setNote(null);
  };

  const empty = !form.title.trim() && !form.username.trim() && !form.password && !form.url.trim();
  const canSave = Boolean(prefill) && !busy && !empty;
  const group = prefill?.groups.find((g) => g.id === form.groupId);

  const save = (typePassword: boolean) => {
    if (!canSave) return;
    setBusy(true);
    setNote(null);
    api.quickSave(form, typePassword && Boolean(prefill?.canType)).then(
      (r) => {
        setBusy(false);
        // typing started: the box is already gone
        if (typePassword && prefill?.canType && !r.typeError) return;
        if (r.typeError) {
          setNote({ text: t('quickSave.typeFailed', { reason: t(`error.${r.typeError.code}`) }), ok: false });
          return;
        }
        if (r.file === 'failed') {
          setNote({ text: t('quickSave.notWritten'), ok: false });
          return;
        }
        setNote({ text: t(r.file === 'pending' ? 'quickSave.pending' : 'quickSave.saved'), ok: true });
        onDone(r.file === 'pending' ? 1600 : 650);
      },
      (e: unknown) => {
        setBusy(false);
        setNote({ text: t(`error.${e instanceof AppError ? e.code : 'internal'}`), ok: false });
      },
    );
  };

  const submit = (e: FormEvent) => {
    e.preventDefault();
    save(false);
  };

  const onKey = (e: KeyboardEvent) => {
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (k === 'Escape') {
      e.preventDefault();
      if (picking) {
        setPicking(false);
        groupButton.current?.focus();
      } else if (generating) {
        setGenerating(false);
        userInput.current?.focus();
      } else onClose();
    } else if (k === 'Enter' && !ctrl && (e.target as Element).matches('.qsv-gen input')) {
      // Enter in the generator's options is not "save"
      e.preventDefault();
    } else if (ctrl && k === 'Enter') {
      e.preventDefault();
      save(Boolean(prefill?.canType));
    } else if (ctrl && k === 'g') {
      e.preventDefault();
      setGenerating((g) => !g);
    }
  };

  const existing = prefill?.existing ?? [];
  const sep = locale === 'zh-CN' ? '、' : ', ';

  return (
    <form className="qsv" onSubmit={submit} onKeyDown={onKey} aria-label={t('quickSave.heading')}>
      <div className="qs-input qsv-head">
        <Icon name="plus" size={20} />
        <span className="qsv-heading">{t('quickSave.heading')}</span>
        <span className="qs-db">
          <Icon name="vault" />
          {status.name}
        </span>
      </div>

      <div className="qsv-fields">
        <div className="ef-two">
          <label className="ef">
            <span>{t('editor.title')}</span>
            <input
              ref={titleInput}
              value={form.title}
              onChange={(e) => set('title', e.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
          </label>
          <label className="ef">
            <span>{t('editor.url')}</span>
            <input
              value={form.url}
              onChange={(e) => set('url', e.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
          </label>
        </div>
        <label className="ef">
          <span>{t('editor.username')}</span>
          <input
            ref={userInput}
            autoFocus
            value={form.username}
            onChange={(e) => set('username', e.target.value)}
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        <div className="ef">
          <span>{t('editor.password')}</span>
          <div className="ef-row ef-box">
            <input
              ref={passwordInput}
              type={showPassword ? 'text' : 'password'}
              value={form.password}
              onChange={(e) => set('password', e.target.value)}
              aria-label={t('editor.password')}
              spellCheck={false}
              autoComplete="off"
            />
            <button
              type="button"
              className="act"
              title={t(showPassword ? 'editor.hide' : 'editor.show')}
              aria-pressed={showPassword}
              onClick={() => setShowPassword((s) => !s)}
            >
              <Icon name={showPassword ? 'eyeOff' : 'eye'} />
            </button>
            <button
              type="button"
              className={generating ? 'act active' : 'act'}
              title={`${t('editor.generate')} (Ctrl+G)`}
              aria-label={t('editor.generate')}
              aria-expanded={generating}
              onClick={() => setGenerating((g) => !g)}
            >
              <Icon name="key" />
            </button>
          </div>
          <StrengthMeter password={form.password} />
        </div>
        {generating && (
          <div className="qsv-gen" role="group" aria-label={t('gen.title')}>
            <GeneratorPanel
              onUse={(pw) => {
                set('password', pw);
                setShowPassword(true);
                setGenerating(false);
                userInput.current?.focus();
              }}
            />
          </div>
        )}
        <div className="ef">
          <span>{t('quickSave.group')}</span>
          <button
            type="button"
            ref={groupButton}
            className="qsv-group"
            aria-expanded={picking}
            disabled={!prefill}
            onClick={() => setPicking((p) => !p)}
          >
            <Icon name="folder" />
            <span className="qsv-group-name">{group?.name ?? '…'}</span>
            <Icon name="chevron" />
          </button>
          {picking && prefill && (
            <GroupList
              groups={prefill.groups}
              current={form.groupId}
              onPick={(id) => {
                setForm((f) => ({ ...f, groupId: id }));
                setPicking(false);
                groupButton.current?.focus();
              }}
            />
          )}
        </div>
        {existing.length > 0 && (
          <div className="qsv-existing">
            <Icon name="warn" size={14} />
            <span>
              {t('quickSave.existing', {
                list:
                  existing
                    .slice(0, EXISTING_SHOWN)
                    .map((e) => (e.username ? `${e.title} (${e.username})` : e.title))
                    .join(sep) +
                  (existing.length > EXISTING_SHOWN ? ` +${existing.length - EXISTING_SHOWN}` : ''),
              })}
            </span>
          </div>
        )}
      </div>

      <footer className="qs-foot qsv-foot">
        {note ? (
          <span className={note.ok ? 'qr-flash ok qsv-note' : 'qr-flash qsv-note'} role="status">
            <Icon name={note.ok ? 'check' : 'warn'} size={14} />
            {note.text}
          </span>
        ) : (
          <>
            <span className="hint">
              <Keys keys={['Enter']} />
              {t('quickSave.save')}
            </span>
            {prefill?.canType && (
              <span className="hint">
                <Keys keys={['Ctrl', 'Enter']} />
                {t('quickSave.saveAndType')}
              </span>
            )}
            <span className="hint">
              <Keys keys={['Esc']} />
              {t('quick.close')}
            </span>
          </>
        )}
        <button type="submit" className="btn primary small qsv-save" disabled={!canSave}>
          {busy ? t('editor.saving') : t('quickSave.save')}
        </button>
      </footer>
    </form>
  );
}

/** The groups, indented by depth; arrow keys move, Enter picks. */
function GroupList({
  groups,
  current,
  onPick,
}: {
  groups: QuickSavePrefill['groups'];
  current: string;
  onPick: (id: string) => void;
}) {
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const on = list.current?.querySelector<HTMLButtonElement>('[aria-selected="true"]');
    on?.focus();
    on?.scrollIntoView({ block: 'nearest' });
  }, []);
  return (
    <div className="qsv-groups" role="listbox" ref={list}>
      {groups.map((g) => (
        <button
          key={g.id}
          type="button"
          role="option"
          aria-selected={g.id === current}
          className={g.id === current ? 'qsv-group-opt on' : 'qsv-group-opt'}
          style={{ paddingLeft: 10 + g.depth * 16 }}
          onClick={() => onPick(g.id)}
          onKeyDown={(e) => {
            const all = [...(list.current?.querySelectorAll<HTMLButtonElement>('button') ?? [])];
            const pos = all.indexOf(e.currentTarget);
            const step = e.key === 'ArrowDown' ? 1 : e.key === 'ArrowUp' ? -1 : 0;
            if (step) {
              e.preventDefault();
              all[Math.max(0, Math.min(all.length - 1, pos + step))]?.focus();
            } else if (e.key === 'Enter' || e.key === ' ') {
              // pick the group, not submit the form
              e.preventDefault();
              e.stopPropagation();
              onPick(g.id);
            }
          }}
        >
          <Icon name="folder" size={16} />
          <span>{g.name}</span>
        </button>
      ))}
    </div>
  );
}
