import { Component, inject, OnInit } from '@angular/core';
import { CharacterEditorStateService } from './character-editor-state.service';
import { CharactersEditorComponent } from './characters-editor/characters-editor.component';
import { CreateAreaModalComponent } from './create-area-modal/create-area-modal.component';
import { EditorEngineService } from './editor-engine.service';
import { EditorStateService } from './editor-state.service';
import { EntityEditorStateService } from './entity-editor-state.service';
import { EntityTemplatesEditorComponent } from './entity-templates-editor/entity-templates-editor.component';
import { ScriptEditorModalComponent } from './script-editor-modal/script-editor-modal.component';
import { EditorToolbarComponent } from './toolbar/editor-toolbar.component';
import { ENTITY_SUBMODES, EntityToolComponent } from './tools/entity-tool/entity-tool.component';
import {
  DECORATION_SUBMODES,
  DecorationToolComponent,
} from './tools/decoration-tool/decoration-tool.component';
import {
  HEIGHT_MODES,
  HeightmapToolComponent,
} from './tools/heightmap-tool/heightmap-tool.component';
import { ROAD_SUBMODES, RoadToolComponent } from './tools/road-tool/road-tool.component';
import { TextureToolComponent } from './tools/texture-tool/texture-tool.component';
import {
  WALKABILITY_MODES,
  WalkabilityToolComponent,
} from './tools/walkability-tool/walkability-tool.component';

const NON_TEXT_INPUT_TYPES = new Set(['range', 'checkbox', 'radio', 'button', 'color']);

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) {
    return false;
  }
  if (target instanceof HTMLElement && target.isContentEditable) {
    return true;
  }
  const tag = target.tagName.toUpperCase();
  if (tag === 'TEXTAREA' || tag === 'SELECT') {
    return true;
  }
  if (tag === 'INPUT') {
    const type = (target.getAttribute('type') ?? '').toLowerCase();
    return !NON_TEXT_INPUT_TYPES.has(type);
  }
  return false;
}

@Component({
  selector: 'app-editor-ui',
  standalone: true,
  imports: [
    EditorToolbarComponent,
    HeightmapToolComponent,
    TextureToolComponent,
    RoadToolComponent,
    DecorationToolComponent,
    EntityToolComponent,
    WalkabilityToolComponent,
    CreateAreaModalComponent,
    CharactersEditorComponent,
    EntityTemplatesEditorComponent,
    ScriptEditorModalComponent,
  ],
  providers: [EditorStateService, CharacterEditorStateService, EntityEditorStateService],
  templateUrl: './editor-ui.component.html',
  styleUrl: './editor-ui.component.css',
  host: {
    '(window:keydown)': 'onWindowKeyDown($event)',
  },
})
export class EditorUiComponent implements OnInit {
  readonly engine = inject(EditorEngineService);
  readonly state = inject(EditorStateService);
  readonly charState = inject(CharacterEditorStateService);
  readonly entityState = inject(EntityEditorStateService);

  async ngOnInit(): Promise<void> {
    await Promise.all([
      this.state.initData(),
      this.charState.initData(),
      this.entityState.initData(),
    ]);
  }

  onWindowKeyDown(ev: KeyboardEvent): void {
    if (ev.ctrlKey || ev.metaKey || ev.altKey || ev.shiftKey || ev.repeat) {
      return;
    }
    if ((this.engine.status()?.editorMode ?? 'world') !== 'world') {
      return;
    }
    if (this.state.showNewAreaModal() || this.entityState.scriptModalTarget() !== null || isEditableTarget(ev.target)) {
      return;
    }
    if (ev.key < '1' || ev.key > '9') {
      return;
    }

    const idx = Number(ev.key) - 1;
    switch (this.state.activeTool()) {
      case 'heightmap': {
        const mode = HEIGHT_MODES[idx];
        if (mode) this.engine.setHeightMode(mode.id);
        break;
      }
      case 'texture': {
        const layer = this.state.terrainTextureLayers()[idx];
        if (layer) this.engine.setTextureLayer(layer.index);
        break;
      }
      case 'road': {
        const submode = ROAD_SUBMODES[idx];
        if (submode) this.engine.setRoadSubmode(submode.id);
        break;
      }
      case 'decoration': {
        const submode = DECORATION_SUBMODES[idx];
        if (submode) this.engine.setDecorationSubmode(submode.id);
        break;
      }
      case 'entities': {
        const submode = ENTITY_SUBMODES[idx];
        if (submode) this.engine.setEntitySubmode(submode.id);
        break;
      }
      case 'walkability': {
        const mode = WALKABILITY_MODES[idx];
        if (mode) this.engine.setWalkabilityMode(mode.id);
        break;
      }
    }
  }
}
