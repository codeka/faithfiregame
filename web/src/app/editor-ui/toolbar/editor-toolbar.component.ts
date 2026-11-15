import { Component, inject } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { AreaMetadata, EngineService, formatAreaKind } from '../../engine.service';
import { EditorStateService } from '../editor-state.service';

@Component({
  selector: 'app-editor-toolbar',
  standalone: true,
  imports: [FormsModule],
  templateUrl: './editor-toolbar.component.html',
  styleUrls: ['../editor-controls.css', './editor-toolbar.component.css'],
})
export class EditorToolbarComponent {
  readonly engine = inject(EngineService);
  readonly state = inject(EditorStateService);

  describeArea(meta: AreaMetadata): string {
    return `${meta.name} (${formatAreaKind(meta)})`;
  }
}
