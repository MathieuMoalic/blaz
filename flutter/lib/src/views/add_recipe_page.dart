import 'package:flutter/material.dart';
import '../api.dart';

class AddRecipePage extends StatefulWidget {
  const AddRecipePage({super.key});
  @override
  State<AddRecipePage> createState() => _AddRecipePageState();
}

class _AddRecipePageState extends State<AddRecipePage> {
  final _controller = TextEditingController();
  bool _busy = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    final title = _controller.text.trim();
    if (title.isEmpty) return;
    setState(() => _busy = true);
    try {
      await createRecipe(title);
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Failed: $e')));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Add recipe')),
      body: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          children: [
            TextField(
              controller: _controller,
              decoration: const InputDecoration(
                labelText: 'Title',
                border: OutlineInputBorder(),
              ),
              onSubmitted: (_) => _submit(),
            ),
            const SizedBox(height: 12),
            FilledButton.icon(
              onPressed: _busy ? null : _submit,
              icon: const Icon(Icons.check),
              label: const Text('Create'),
            ),
          ],
        ),
      ),
    );
  }
}
