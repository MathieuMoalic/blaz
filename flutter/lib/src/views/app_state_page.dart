import 'package:flutter/material.dart';
import '../api.dart' as api;
import '../auth.dart';

class AppStatePage extends StatefulWidget {
  const AppStatePage({super.key});

  @override
  State<AppStatePage> createState() => _AppStatePageState();
}

class _AppStatePageState extends State<AppStatePage> {
  final _form = GlobalKey<FormState>();

  final _apiKeyCtrl = TextEditingController();
  final _modelCtrl = TextEditingController();
  final _apiUrlCtrl = TextEditingController();
  final _sysImportCtrl = TextEditingController();
  final _sysMacrosCtrl = TextEditingController();

  bool _allowRegistration = true;
  bool _busy = false;
  bool _obscureKey = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _apiKeyCtrl.dispose();
    _modelCtrl.dispose();
    _apiUrlCtrl.dispose();
    _sysImportCtrl.dispose();
    _sysMacrosCtrl.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() => _busy = true);
    try {
      final s = await api.fetchAppSettings();
      _apiKeyCtrl.text = s.llmApiKey ?? '';
      _modelCtrl.text = s.llmModel;
      _apiUrlCtrl.text = s.llmApiUrl;
      _allowRegistration = s.allowRegistration;
      _sysImportCtrl.text = s.systemPromptImport;
      _sysMacrosCtrl.text = s.systemPromptMacros;
      setState(() {});
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Failed to load: $e')));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _save() async {
    if (!_form.currentState!.validate()) return;

    setState(() => _busy = true);
    try {
      final updated = await api.updateAppSettings(
        api.AppSettings(
          llmApiKey: _apiKeyCtrl.text.trim().isEmpty
              ? null
              : _apiKeyCtrl.text.trim(),
          llmModel: _modelCtrl.text.trim(),
          llmApiUrl: _apiUrlCtrl.text.trim(),
          allowRegistration: _allowRegistration,
          systemPromptImport: _sysImportCtrl.text,
          systemPromptMacros: _sysMacrosCtrl.text,
        ),
      );

      // Keep login UI logic in sync without needing an app restart.
      Auth.allowRegistration = updated.allowRegistration;

      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(const SnackBar(content: Text('Settings saved')));
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Save failed: $e')));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final gap = const SizedBox(height: 12);

    return Scaffold(
      appBar: AppBar(title: const Text('App settings')),
      body: AbsorbPointer(
        absorbing: _busy,
        child: Stack(
          children: [
            Form(
              key: _form,
              child: ListView(
                padding: const EdgeInsets.all(16),
                children: [
                  // LLM
                  Text(
                    'Language Model',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  gap,
                  TextFormField(
                    controller: _apiKeyCtrl,
                    obscureText: _obscureKey,
                    decoration: InputDecoration(
                      labelText: 'LLM API key',
                      border: const OutlineInputBorder(),
                      suffixIcon: IconButton(
                        tooltip: _obscureKey ? 'Show' : 'Hide',
                        icon: Icon(
                          _obscureKey ? Icons.visibility : Icons.visibility_off,
                        ),
                        onPressed: () =>
                            setState(() => _obscureKey = !_obscureKey),
                      ),
                      hintText: 'sk-… or or-…',
                    ),
                  ),
                  gap,
                  TextFormField(
                    controller: _modelCtrl,
                    decoration: const InputDecoration(
                      labelText: 'LLM model',
                      border: OutlineInputBorder(),
                      hintText: 'e.g. deepseek/deepseek-chat-v3.1',
                    ),
                    validator: (v) => (v == null || v.trim().isEmpty)
                        ? 'Model required'
                        : null,
                  ),
                  gap,
                  TextFormField(
                    controller: _apiUrlCtrl,
                    decoration: const InputDecoration(
                      labelText: 'LLM API base URL',
                      border: OutlineInputBorder(),
                      hintText: 'e.g. https://openrouter.ai/api/v1',
                    ),
                    validator: (v) => (v == null || v.trim().isEmpty)
                        ? 'API URL required'
                        : null,
                  ),
                  const SizedBox(height: 20),

                  // Registration toggle
                  SwitchListTile(
                    title: const Text('Allow new user registration'),
                    value: _allowRegistration,
                    onChanged: (v) => setState(() => _allowRegistration = v),
                  ),
                  const SizedBox(height: 20),

                  // Prompts
                  Text(
                    'System prompts',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: 8),
                  TextField(
                    controller: _sysImportCtrl,
                    maxLines: 8,
                    decoration: const InputDecoration(
                      labelText: 'Import system prompt',
                      alignLabelWithHint: true,
                      border: OutlineInputBorder(),
                    ),
                  ),
                  gap,
                  TextField(
                    controller: _sysMacrosCtrl,
                    maxLines: 10,
                    decoration: const InputDecoration(
                      labelText: 'Macros system prompt',
                      alignLabelWithHint: true,
                      border: OutlineInputBorder(),
                    ),
                  ),
                  const SizedBox(height: 16),
                  FilledButton.icon(
                    onPressed: _save,
                    icon: _busy
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.save_outlined),
                    label: const Text('Save settings'),
                  ),
                ],
              ),
            ),
            if (_busy)
              const Positioned.fill(
                child: IgnorePointer(
                  child: Center(child: CircularProgressIndicator()),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
