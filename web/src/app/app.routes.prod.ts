import { Routes } from '@angular/router';
import { HudComponent } from './hud/hud.component';

export const routes: Routes = [
  { path: '', component: HudComponent },
  { path: 'game', component: HudComponent },
  { path: '**', redirectTo: '' },
];
