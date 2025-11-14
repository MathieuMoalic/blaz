import 'package:flutter/material.dart';
import '../auth.dart';
import '../home_shell.dart';
import 'server_url_dialog.dart'; // <-- NEW

class LoginPage extends StatefulWidget {
  const LoginPage({super.key});
  @override
  State<LoginPage> createState() => _LoginPageState();
}

class _LoginPageState extends State<LoginPage> {
  final _form = GlobalKey<FormState>();
  final _email = TextEditingController();
  final _password = TextEditingController();
  bool _busy = false;
  bool _registerMode = false;

  // Require a successful server check per session before attempting auth.
  bool _serverVerifiedThisSession = false;

  @override
  void initState() {
    super.initState();
    // On first boot (no user on server), this will be true and we show
    // the "Create account" flow by default. Once a user exists, it's false,
    // and this page is pure "Sign in".
    _registerMode = Auth.allowRegistration;
  }

  @override
  void dispose() {
    _email.dispose();
    _password.dispose();
    super.dispose();
  }

  Future<bool> _ensureServerUrl() async {
    if (_serverVerifiedThisSession) return true;
    final ok =
        await showDialog<bool>(
          context: context,
          barrierDismissible: false,
          builder: (_) => const ServerUrlDialog(),
        ) ??
        false;
    if (ok) _serverVerifiedThisSession = true;
    return ok;
  }

  Future<void> _submit() async {
    // 1) Ask for server URL & verify /healthz (saves URL on success)
    final serverOk = await _ensureServerUrl();
    if (!serverOk) return;

    // 2) Validate credentials
    if (!_form.currentState!.validate()) return;

    setState(() => _busy = true);
    try {
      if (_registerMode) {
        await Auth.register(
          email: _email.text.trim(),
          password: _password.text,
        );
        if (!mounted) return;
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Account created. Please sign in.')),
        );
      }

      await Auth.login(email: _email.text.trim(), password: _password.text);
      if (!mounted) return;
      Navigator.of(context).pushAndRemoveUntil(
        MaterialPageRoute(builder: (_) => const HomeShell()),
        (_) => false,
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Auth failed: $e')));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _changeServerUrl() async {
    final ok =
        await showDialog<bool>(
          context: context,
          barrierDismissible: true,
          builder: (_) => const ServerUrlDialog(),
        ) ??
        false;
    if (ok) {
      // Mark as verified for this session so we don't prompt again immediately.
      setState(() => _serverVerifiedThisSession = true);
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(const SnackBar(content: Text('Server URL saved.')));
    }
  }

  @override
  Widget build(BuildContext context) {
    final canRegister = Auth.allowRegistration;

    return Scaffold(
      appBar: AppBar(
        title: Text(_registerMode ? 'Create account' : 'Sign in'),
        actions: [
          IconButton(
            tooltip: 'Server URL',
            onPressed: _busy ? null : _changeServerUrl,
            icon: const Icon(Icons.cloud),
          ),
        ],
      ),
      body: SafeArea(
        child: Form(
          key: _form,
          child: ListView(
            padding: const EdgeInsets.all(16),
            children: [
              TextFormField(
                controller: _email,
                decoration: const InputDecoration(
                  labelText: 'Email',
                  border: OutlineInputBorder(),
                ),
                keyboardType: TextInputType.emailAddress,
                validator:
                    (v) =>
                        (v == null || !v.contains('@'))
                            ? 'Enter a valid email'
                            : null,
              ),
              const SizedBox(height: 12),
              TextFormField(
                controller: _password,
                decoration: const InputDecoration(
                  labelText: 'Password',
                  border: OutlineInputBorder(),
                ),
                obscureText: true,
                validator:
                    (v) =>
                        (v == null || v.length < 8) ? 'Min 8 characters' : null,
                onFieldSubmitted: (_) => _submit(),
              ),
              const SizedBox(height: 16),
              FilledButton.icon(
                onPressed: _busy ? null : _submit,
                icon:
                    _busy
                        ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                        : const Icon(Icons.lock_open),
                label: Text(_registerMode ? 'Register & Sign in' : 'Sign in'),
              ),
              const SizedBox(height: 8),
              if (canRegister)
                TextButton(
                  onPressed:
                      _busy
                          ? null
                          : () =>
                              setState(() => _registerMode = !_registerMode),
                  child: Text(
                    _registerMode
                        ? 'I already have an account'
                        : 'Create an account',
                  ),
                )
              else
                Text(
                  'Registration is disabled by the administrator.',
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: Theme.of(context).colorScheme.onSurfaceVariant,
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
