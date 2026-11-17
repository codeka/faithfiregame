import { Routes } from '@angular/router';
import { HudComponent } from './hud/hud.component';
import { LoginComponent } from './login/login.component';

export const routes: Routes = [
  { path: '', component: HudComponent },
  { path: 'login', component: LoginComponent },
  { path: 'game', redirectTo: '', pathMatch: 'full' },
  { path: '**', redirectTo: '' },
];
