import { Routes } from '@angular/router';
import { EditorUiComponent } from './editor-ui/editor-ui.component';
import { HudComponent } from './hud/hud.component';
import { LoginComponent } from './login/login.component';

export const routes: Routes = [
  { path: '', component: HudComponent },
  { path: 'login', component: LoginComponent },
  { path: 'game', redirectTo: '', pathMatch: 'full' },
  { path: 'editor', component: EditorUiComponent },
  { path: '**', redirectTo: '' },
];
