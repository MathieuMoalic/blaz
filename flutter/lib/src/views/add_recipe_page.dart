import 'package:flutter/material.dart';
import 'package:file_selector/file_selector.dart';
import 'package:path/path.dart' as p;
import 'dart:io' show File;
import 'dart:typed_data';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:path_provider/path_provider.dart';
import '../api.dart';

class AddRecipePage extends StatefulWidget {
  const AddRecipePage({super.key});
  @override
  State<AddRecipePage> createState() => _AddRecipePageState();
}

class _AddRecipePageState extends State<AddRecipePage> {
  final _form = GlobalKey<FormState>();

  final _title = TextEditingController();
  final _source = TextEditingController();
  final _yieldText = TextEditingController();
  final _notes = TextEditingController();
  final _ingredientsRaw = TextEditingController();
  final _instructionsRaw = TextEditingController();

  XFile? _picked; // selected file
  Uint8List? _preview; // preview bytes (web or when Android only returns URI)
  bool _busy = false;

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
  Future<void> _pickImage() async {
    final group = const XTypeGroup(
      label: 'images',
      extensions: ['jpg', 'jpeg', 'png', 'webp', 'gif'],
    );

    final x = await openFile(acceptedTypeGroups: [group]);
    if (x == null) return;

    // Always read bytes for consistent uploads + preview
    final bytes = await x.readAsBytes();

    setState(() {
      _picked = x;
      _preview = bytes;
    });
  }

  Future<void> _submit() async {
    if (!_form.currentState!.validate()) return;

    final title = _title.text.trim();
    final source = _source.text.trim();
    final yieldText = _yieldText.text.trim();
    final notes = _notes.text.trim();
    final ingredients = _lines(_ingredientsRaw.text);
    final instructions = _lines(_instructionsRaw.text);

    setState(() => _busy = true);
    try {
      final created = await createRecipeFull(
        title: title,
        source: source,
        yieldText: yieldText,
        notes: notes,
        ingredients: ingredients,
        instructions: instructions,
      );

      if (_picked != null) {
        final name = _picked!.name.isNotEmpty
            ? _picked!.name
            : (_picked!.path.isNotEmpty ? p.basename(_picked!.path) : 'upload');

        if (_preview != null) {
          await uploadRecipeImage(
            id: created.id,
            filename: name,
            bytes: _preview!, // <-- use bytes on Android/iOS too
          );
        } else if (_picked!.path.isNotEmpty) {
          await uploadRecipeImage(
            id: created.id,
            path: _picked!.path, // fallback if for some reason bytes missing
            filename: name,
          );
        } else {
          // Extremely rare: neither bytes nor path — show a friendly error
          throw Exception('No image data available from the picker');
        }
      }

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
    final gap = const SizedBox(height: 12);

    final selectedName = _picked == null
        ? 'No image selected'
        : (_picked!.name.isNotEmpty
              ? _picked!.name
              : (_picked!.path.isNotEmpty
                    ? p.basename(_picked!.path)
                    : 'Selected image'));

    final hasBytesPreview = _preview != null;
    final hasFilePreview =
        !kIsWeb && _picked != null && _picked!.path.isNotEmpty;

    return Scaffold(
      appBar: AppBar(title: const Text('Add recipe')),
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
                autofillHints: const <String>[], // silence Autofill logs
                validator: (v) =>
                    (v == null || v.trim().isEmpty) ? 'Title required' : null,
              ),
              gap,
              Row(
                children: [
                  FilledButton.icon(
                    onPressed: _busy ? null : _pickImage,
                    icon: const Icon(Icons.photo),
                    label: const Text('Choose image'),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(selectedName, overflow: TextOverflow.ellipsis),
                  ),
                ],
              ),

              if (hasBytesPreview || hasFilePreview) ...[
                const SizedBox(height: 8),
                SizedBox(
                  height: 120,
                  child: ClipRRect(
                    borderRadius: BorderRadius.circular(8),
                    child: hasBytesPreview
                        ? Image.memory(_preview!, fit: BoxFit.cover)
                        : Image.file(File(_picked!.path), fit: BoxFit.cover),
                  ),
                ),
              ],

              gap,
              TextField(
                controller: _source,
                decoration: const InputDecoration(
                  labelText: 'Source (URL, book, person…)',
                  border: OutlineInputBorder(),
                ),
                textInputAction: TextInputAction.next,
                autofillHints: const <String>[],
              ),
              gap,
              TextField(
                controller: _yieldText,
                decoration: const InputDecoration(
                  labelText: 'Yield (e.g. “4 servings”)',
                  border: OutlineInputBorder(),
                ),
                textInputAction: TextInputAction.next,
                autofillHints: const <String>[],
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
                autofillHints: const <String>[],
              ),
              gap,
              TextField(
                controller: _ingredientsRaw,
                decoration: const InputDecoration(
                  labelText: 'Ingredients (one per line)',
                  hintText: 'e.g.\n2 eggs\n150 g flour\nPinch of salt',
                  border: OutlineInputBorder(),
                  alignLabelWithHint: true,
                ),
                maxLines: 6,
                autofillHints: const <String>[],
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
                maxLines: 8,
                autofillHints: const <String>[],
              ),
              const SizedBox(height: 16),
              FilledButton.icon(
                onPressed: _busy ? null : _submit,
                icon: _busy
                    ? const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.check),
                label: const Text('Create'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
