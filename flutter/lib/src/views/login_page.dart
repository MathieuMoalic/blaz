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
  final _password = TextEditingController();
  bool _busy = false;

  // Require a successful server check per session before attempting auth.
  bool _serverVerifiedThisSession = false;

  @override
  void dispose() {
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
      await Auth.login(password: _password.text);
      if (!mounted) return;
      Navigator.of(context).pushAndRemoveUntil(
        MaterialPageRoute(builder: (_) => const HomeShell()),
        (_) => false,
      );
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Login failed: $e')));
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
    return Scaffold(
      appBar: AppBar(
        title: const Text('Sign in'),
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
                controller: _password,
                decoration: const InputDecoration(
                  labelText: 'Password',
                  border: OutlineInputBorder(),
                ),
                obscureText: true,
                validator: (v) => (v == null || v.isEmpty) ? 'Enter password' : null,
                onFieldSubmitted: (_) => _submit(),
                autofocus: true,
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
                label: const Text('Sign in'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
