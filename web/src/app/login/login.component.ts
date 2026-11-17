import { Component, inject, OnInit, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';
import { AuthService } from '../auth/auth.service';
import { EngineService } from '../engine.service';

@Component({
  selector: 'app-login',
  standalone: true,
  imports: [FormsModule, RouterLink],
  templateUrl: './login.component.html',
  styleUrl: './login.component.css',
})
export class LoginComponent implements OnInit {
  private readonly auth = inject(AuthService);
  private readonly engine = inject(EngineService);
  private readonly router = inject(Router);

  username = '';
  password = '';
  readonly submitting = signal(false);
  readonly errorMessage = signal<string | null>(null);

  ngOnInit(): void {
    this.engine.enterLandingMode();
  }

  async onSubmit(): Promise<void> {
    if (this.submitting()) {
      return;
    }
    const cleanUser = this.username.trim();
    if (!cleanUser || !this.password) {
      this.errorMessage.set('Please enter both your username and password.');
      return;
    }
    this.errorMessage.set(null);
    this.submitting.set(true);
    try {
      await this.auth.login(cleanUser, this.password);
      await this.router.navigate(['/']);
    } catch (err) {
      this.errorMessage.set(err instanceof Error ? err.message : String(err));
    } finally {
      this.submitting.set(false);
    }
  }
}
