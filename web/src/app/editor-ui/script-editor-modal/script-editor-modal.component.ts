import {
  AfterViewInit,
  Component,
  computed,
  ElementRef,
  inject,
  OnDestroy,
  OnInit,
  signal,
  ViewChild,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { EditorEngineService, EntityOverrides } from '../editor-engine.service';
import { EditorStateService } from '../editor-state.service';
import { EntityEditorStateService, resolvePlacedEntity } from '../entity-editor-state.service';
import {
  buildFaithFireTypeDefs,
  DEFAULT_TRIGGER_SCRIPT_SOURCE,
  SCRIPT_SNIPPETS,
  transpileTypeScriptToJs,
} from '../script-compiler';

interface MonacoDisposable {
  dispose(): void;
}

interface MonacoCodeEditor extends MonacoDisposable {
  getValue(): string;
  setValue(val: string): void;
  onDidChangeModelContent(listener: () => void): MonacoDisposable;
  addCommand(keybinding: number, handler: () => void): void;
  layout(): void;
}

interface MonacoApi {
  KeyMod: { CtrlCmd: number };
  KeyCode: { KeyS: number };
  editor: {
    defineTheme(name: string, theme: unknown): void;
    create(container: HTMLElement, options: unknown): MonacoCodeEditor;
    getModelMarkers(filter: { owner?: string }): Array<{ severity: number; startLineNumber: number; startColumn: number; message: string }>;
  };
  languages: {
    typescript: {
      ScriptTarget: { ES2022: number; ESNext: number };
      typescriptDefaults: {
        setCompilerOptions(opts: unknown): void;
        setExtraLibs(libs: Array<{ content: string; filePath?: string }>): void;
      };
    };
  };
}

let cachedMonacoPromise: Promise<MonacoApi> | null = null;

function loadMonacoAmd(): Promise<MonacoApi> {
  const win = window as unknown as {
    monaco?: MonacoApi;
    require?: {
      (deps: string[], onLoad: (mod: MonacoApi) => void, onError?: (err: unknown) => void): void;
      config(cfg: { paths: Record<string, string> }): void;
    };
  };
  if (win.monaco) {
    return Promise.resolve(win.monaco);
  }
  if (cachedMonacoPromise) {
    return cachedMonacoPromise;
  }

  cachedMonacoPromise = new Promise<MonacoApi>((resolve, reject) => {
    const startAmd = () => {
      if (!win.require) {
        reject(new Error('Monaco AMD loader missing window.require'));
        return;
      }
      win.require.config({ paths: { vs: '/assets/monaco/vs' } });
      win.require(
        ['vs/editor/editor.main'],
        (monacoMod) => resolve(monacoMod ?? win.monaco!),
        (err) => reject(err),
      );
    };

    const existing = document.querySelector('script[data-monaco-loader="true"]');
    if (existing) {
      if (win.require) {
        startAmd();
      } else {
        existing.addEventListener('load', startAmd, { once: true });
        existing.addEventListener('error', () => reject(new Error('Failed to load Monaco loader.js')), {
          once: true,
        });
      }
      return;
    }

    const script = document.createElement('script');
    script.src = '/assets/monaco/vs/loader.js';
    script.async = true;
    script.setAttribute('data-monaco-loader', 'true');
    script.onload = startAmd;
    script.onerror = () => reject(new Error('Failed to load /assets/monaco/vs/loader.js'));
    document.head.appendChild(script);
  });

  return cachedMonacoPromise;
}

@Component({
  selector: 'app-script-editor-modal',
  standalone: true,
  imports: [FormsModule],
  templateUrl: './script-editor-modal.component.html',
  styleUrls: ['../editor-controls.css', './script-editor-modal.component.css'],
  host: {
    '(window:keydown.escape)': 'onEscape()',
  },
})
export class ScriptEditorModalComponent implements OnInit, AfterViewInit, OnDestroy {
  readonly engine = inject(EditorEngineService);
  readonly worldState = inject(EditorStateService);
  readonly entityState = inject(EntityEditorStateService);

  @ViewChild('monacoHost')
  private monacoHostRef?: ElementRef<HTMLDivElement>;

  readonly snippets = SCRIPT_SNIPPETS;
  readonly selectedSnippetId = signal<string>(SCRIPT_SNIPPETS[0].id);
  readonly draftSource = signal<string>(DEFAULT_TRIGGER_SCRIPT_SOURCE);
  readonly monacoReady = signal<boolean>(false);
  readonly monacoError = signal<string | null>(null);
  readonly monacoMarkerErrors = signal<string[]>([]);

  private editorInstance: MonacoCodeEditor | null = null;
  private changeSub: MonacoDisposable | null = null;
  private resizeObserver: ResizeObserver | null = null;
  private destroyed = false;

  readonly targetTitle = computed(() => {
    const target = this.entityState.scriptModalTarget();
    if (!target) return 'Entity Script';
    if (target.kind === 'template') {
      const tpl = this.entityState.activeTemplate();
      return tpl ? `Template Script · ${tpl.name}` : 'Template Script';
    }
    const sel = this.engine.status()?.selectedEntity;
    return sel ? `Instance Script · #${sel.id} (${sel.template_id})` : 'Instance Script';
  });

  readonly transpilePreview = computed(() => transpileTypeScriptToJs(this.draftSource()));
  readonly diagnostics = computed(() => [
    ...this.transpilePreview().diagnostics,
    ...this.monacoMarkerErrors(),
  ]);

  ngOnInit(): void {
    const target = this.entityState.scriptModalTarget();
    if (!target) return;
    if (target.kind === 'template') {
      const src = this.entityState.activeTemplate()?.script?.source;
      this.draftSource.set(src && src.length > 0 ? src : DEFAULT_TRIGGER_SCRIPT_SOURCE);
      return;
    }
    const sel = this.engine.status()?.selectedEntity;
    if (sel) {
      const tpl = this.entityState.templates().find((t) => t.id === sel.template_id) ?? null;
      const res = resolvePlacedEntity(sel, tpl);
      const src = res.script?.source;
      this.draftSource.set(src && src.length > 0 ? src : DEFAULT_TRIGGER_SCRIPT_SOURCE);
    }
  }

  ngAfterViewInit(): void {
    const isJsdom =
      typeof navigator !== 'undefined' && /jsdom/i.test(navigator.userAgent ?? '');
    if (isJsdom) {
      return;
    }
    void this.initMonaco();
  }

  private async initMonaco(): Promise<void> {
    try {
      const monaco = await loadMonacoAmd();
      if (this.destroyed) return;
      const host = this.monacoHostRef?.nativeElement;
      if (!host) return;

      const areaIds = this.worldState.areas().map((a) => a.id);
      const tplIds = this.entityState.templates().map((t) => t.id);
      const typeDefs = buildFaithFireTypeDefs(areaIds, tplIds);

      monaco.languages.typescript.typescriptDefaults.setCompilerOptions({
        target: monaco.languages.typescript.ScriptTarget.ES2022,
        allowNonTsExtensions: true,
        checkJs: false,
      });
      monaco.languages.typescript.typescriptDefaults.setExtraLibs([
        { content: typeDefs, filePath: 'file:///faithfire.d.ts' },
      ]);

      monaco.editor.defineTheme('cryo-dark', {
        base: 'vs-dark',
        inherit: true,
        rules: [
          { token: 'keyword', foreground: '63b3ed' },
          { token: 'type.identifier', foreground: '8ef7ff' },
          { token: 'string', foreground: '9effde' },
        ],
        colors: {
          'editor.background': '#0d131c',
          'editor.lineHighlightBackground': '#142030',
          'editorCursor.foreground': '#00e5ff',
        },
      });

      const editor = monaco.editor.create(host, {
        value: this.draftSource(),
        language: 'typescript',
        theme: 'cryo-dark',
        automaticLayout: true,
        minimap: { enabled: false },
        fontSize: 13,
        tabSize: 2,
        scrollBeyondLastLine: false,
        quickSuggestions: true,
      });

      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => this.apply());
      this.changeSub = editor.onDidChangeModelContent(() => {
        this.draftSource.set(editor.getValue());
        const markers = monaco.editor
          .getModelMarkers({ owner: 'typescript' })
          .filter((m) => m.severity >= 8)
          .slice(0, 6)
          .map((m) => `Line ${m.startLineNumber}:${m.startColumn}: ${m.message}`);
        this.monacoMarkerErrors.set(markers);
      });

      if (typeof ResizeObserver !== 'undefined') {
        this.resizeObserver = new ResizeObserver(() => editor.layout());
        this.resizeObserver.observe(host);
      }

      this.editorInstance = editor;
      this.monacoReady.set(true);
    } catch (err) {
      if (!this.destroyed) {
        this.monacoError.set(err instanceof Error ? err.message : String(err));
      }
    }
  }

  onSourceTextareaChange(val: string): void {
    this.draftSource.set(val);
    if (this.editorInstance && this.editorInstance.getValue() !== val) {
      this.editorInstance.setValue(val);
    }
  }

  applySnippet(): void {
    const snippet = this.snippets.find((s) => s.id === this.selectedSnippetId());
    if (!snippet) return;
    this.draftSource.set(snippet.source);
    if (this.editorInstance) {
      this.editorInstance.setValue(snippet.source);
    }
  }

  apply(): void {
    const source = this.editorInstance ? this.editorInstance.getValue() : this.draftSource();
    const { compiledJs, diagnostics } = transpileTypeScriptToJs(source);
    if (diagnostics.length > 0) {
      return;
    }
    const target = this.entityState.scriptModalTarget();
    if (!target) return;
    const payload = { source, compiled_js: compiledJs };

    if (target.kind === 'template') {
      this.entityState.setScript(payload);
    } else {
      const sel = this.engine.status()?.selectedEntity;
      if (sel) {
        const nextOv: EntityOverrides = {
          ...structuredClone(sel.overrides),
          script: { set: payload },
        };
        this.engine.updateSelectedPlacedEntity({ ...sel, overrides: nextOv });
      }
    }
    this.entityState.closeScriptModal();
  }

  onEscape(): void {
    this.entityState.closeScriptModal();
  }

  ngOnDestroy(): void {
    this.destroyed = true;
    this.changeSub?.dispose();
    this.changeSub = null;
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
    this.editorInstance?.dispose();
    this.editorInstance = null;
  }
}
