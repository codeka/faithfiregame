import { Routes } from '@angular/router';
import { EditorUiComponent } from './editor-ui/editor-ui.component';
import { HudComponent } from './hud/hud.component';

export const routes: Routes = [
  { path: '', redirectTo: 'editor', pathMatch: 'full' },
  { path: 'editor', component: EditorUiComponent },
  { path: 'game', component: HudComponent },
];
