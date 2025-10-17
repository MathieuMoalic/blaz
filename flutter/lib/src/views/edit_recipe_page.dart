import 'package:flutter/material.dart';
import 'package:file_selector/file_selector.dart';
import '../api.dart';

class EditRecipePage extends StatefulWidget {
  final Recipe recipe;
  const EditRecipePage({super.key, required this.recipe});

  @override
  State<EditRecipePage> createState() => _EditRecipePageState();
}

class _EditRecipePageState extends State<EditRecipePage> {
  final _form = GlobalKey<FormState>();

  late final TextEditingController _title;
  late final TextEditingController _source;
  late final TextEditingController _yieldText;
  late final TextEditingController _notes;
  late final TextEditingController _ingredientsRaw;
  late final TextEditingController _instructionsRaw;

  bool _busy = false;

  @override
  void initState() {
    super.initState();
    final r = widget.recipe;

    _title = TextEditingController(text: r.title);
    _source = TextEditingController(text: r.source);
    _yieldText = TextEditingController(text: r.yieldText);
    _notes = TextEditingController(text: r.notes);
    _ingredientsRaw = TextEditingController(
      text: r.ingredients.map((ing) => ing.toLine()).join('\n'),
    );
    _instructionsRaw = TextEditingController(text: r.instructions.join('\n'));
  }

  @override
  void dispose() {
    _title.dispose();
    _source.dispose();
    _yieldText.dispose();
    _notes.dispose();
    _ingredientsRaw.dispose();
    _instructionsRaw.dispose();
    super.dispose();
  }

  List<String> _lines(String s) =>
      s.split('\n').map((e) => e.trim()).where((e) => e.isNotEmpty).toList();

  Future<void> _save() async {
    if (!_form.currentState!.validate()) return;

    setState(() => _busy = true);
    try {
      await updateRecipe(
        id: widget.recipe.id,
        title: _title.text.trim(),
        source: _source.text.trim(),
        yieldText: _yieldText.text.trim(),
        notes: _notes.text.trim(),
        // Send raw lines; backend parses to structured
        ingredients: _lines(_ingredientsRaw.text),
        instructions: _lines(_instructionsRaw.text),
      );
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

  Future<void> _changeImage() async {
    final typeGroup = const XTypeGroup(
      label: 'images',
      extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'],
    );
    final file = await openFile(acceptedTypeGroups: [typeGroup]);
    if (file == null) return;

    setState(() => _busy = true);
    try {
      final bytes = await file.readAsBytes(); // works web & native
      await uploadRecipeImage(
        id: widget.recipe.id,
        filename: file.name,
        bytes: bytes,
      );
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(const SnackBar(content: Text('Image updated')));
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Image failed: $e')));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final gap = const SizedBox(height: 12);

    return Scaffold(
      appBar: AppBar(
        title: const Text('Edit recipe'),
        actions: [
          TextButton.icon(
            onPressed: _busy ? null : _changeImage,
            icon: const Icon(Icons.photo_outlined),
            label: const Text('Change image'),
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: SafeArea(
        child: Form(
          key: _form,
          child: ListView(
            padding: const EdgeInsets.all(16),
            children: [
              TextFormField(
                controller: _title,
                decoration: const InputDecoration(
                  labelText: 'Title *',
                  border: OutlineInputBorder(),
                ),
                textInputAction: TextInputAction.next,
                validator: (v) =>
                    (v == null || v.trim().isEmpty) ? 'Title required' : null,
              ),
              gap,
              TextField(
                controller: _source,
                decoration: const InputDecoration(
                  labelText: 'Source',
                  border: OutlineInputBorder(),
                ),
                textInputAction: TextInputAction.next,
              ),
              gap,
              TextField(
                controller: _yieldText,
                decoration: const InputDecoration(
                  labelText: 'Yield',
                  border: OutlineInputBorder(),
                ),
                textInputAction: TextInputAction.next,
              ),
              gap,
              TextField(
                controller: _notes,
                decoration: const InputDecoration(
                  labelText: 'Notes',
                  border: OutlineInputBorder(),
                  alignLabelWithHint: true,
                ),
                maxLines: 4,
              ),
              gap,
              TextField(
                controller: _ingredientsRaw,
                decoration: const InputDecoration(
                  labelText: 'Ingredients (one per line)',
                  hintText: 'e.g.\n150 g flour\n2 carrots, diced',
                  border: OutlineInputBorder(),
                  alignLabelWithHint: true,
                ),
                maxLines: 8,
              ),
              gap,
              TextField(
                controller: _instructionsRaw,
                decoration: const InputDecoration(
                  labelText: 'Instructions (one step per line)',
                  hintText: 'e.g.\nWhisk eggs.\nFold in flour.\nBake 20 min.',
                  border: OutlineInputBorder(),
                  alignLabelWithHint: true,
                ),
                maxLines: 10,
              ),
              const SizedBox(height: 16),
              FilledButton.icon(
                onPressed: _busy ? null : _save,
                icon: _busy
                    ? const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.check),
                label: const Text('Save'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
