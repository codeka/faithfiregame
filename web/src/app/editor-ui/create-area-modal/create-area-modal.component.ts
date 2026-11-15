import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { AreaMetadata } from '../editor-engine.service';
import { EditorStateService } from '../editor-state.service';

@Component({
  selector: 'app-create-area-modal',
  standalone: true,
  imports: [FormsModule],
  templateUrl: './create-area-modal.component.html',
  styleUrls: ['../editor-controls.css', './create-area-modal.component.css'],
})
export class CreateAreaModalComponent {
  readonly state = inject(EditorStateService);

  readonly newAreaId = signal('');
  readonly newAreaName = signal('');
  readonly newAreaKind = signal<'outside' | 'inside_ephemeral' | 'inside_permanent'>('outside');
  readonly newAreaWidth = signal(32);
  readonly newAreaLength = signal(32);
  readonly newAreaError = signal<string | null>(null);
  readonly creatingArea = signal(false);

  async createNewArea(): Promise<void> {
    this.newAreaError.set(null);
    const id = this.newAreaId().trim().toLowerCase();
    const name = this.newAreaName().trim();
    if (!id || !name) {
      this.newAreaError.set('Area ID and Name are required.');
      return;
    }

    this.creatingArea.set(true);
    try {
      const kind = this.newAreaKind();
      const body: Record<string, unknown> = { id, name, kind };
      if (kind !== 'outside') {
        body['width'] = Number(this.newAreaWidth());
        body['length'] = Number(this.newAreaLength());
      }

      const resp = await fetch('/api/editor/areas', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      if (!resp.ok) {
        const text = await resp.text();
        this.newAreaError.set(text || `HTTP ${resp.status}`);
        return;
      }

      const created = (await resp.json()) as AreaMetadata;
      await this.state.handleAreaCreated(created);
    } catch (e) {
      this.newAreaError.set(e instanceof Error ? e.message : String(e));
    } finally {
      this.creatingArea.set(false);
    }
  }
}
