import { Component, computed, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { CharacterEditorStateService } from '../character-editor-state.service';
import {
  AreaMetadata,
  EditorEngineService,
  EditorMode,
  formatAreaKind,
} from '../editor-engine.service';
import { EditorStateService } from '../editor-state.service';

@Component({
  selector: 'app-editor-toolbar',
  standalone: true,
  imports: [FormsModule],
  templateUrl: './editor-toolbar.component.html',
  styleUrls: ['../editor-controls.css', './editor-toolbar.component.css'],
})
export class EditorToolbarComponent {
  readonly engine = inject(EditorEngineService);
  readonly state = inject(EditorStateService);
  readonly charState = inject(CharacterEditorStateService);

  readonly editorMode = computed<EditorMode>(() => this.engine.status()?.editorMode ?? 'world');
  readonly charStudio = computed(() => this.engine.status()?.characterStudio ?? null);

  describeArea(meta: AreaMetadata): string {
    return `${meta.name} (${formatAreaKind(meta)})`;
  }
}
