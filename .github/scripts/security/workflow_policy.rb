#!/usr/bin/env ruby
# frozen_string_literal: true

# Workflow trust policy evaluated over parsed YAML, not text: block and
# inline (flow) forms, quoted keys, `on` parsed as a boolean key, anchors and
# aliases are all the same document to the parser. Usage:
#   workflow_policy.rb <dir>        every *.yml / *.yaml in <dir>
# Prints one violation per line; exit 0 only when the directory holds at
# least one workflow and none violates. A file that does not parse is a
# violation, never a pass.
#
# Violations (matched by the negative controls):
#   no top-level permissions | write permission | privileged trigger |
#   secret exposed | not pinned | persists credentials | unparseable

require 'yaml'
require 'digest'

PINNED = %r{\A[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40}\z}.freeze
LEVELS_OK = %w[read none].freeze
PRIVILEGED = %w[pull_request_target workflow_run].freeze

def parse(text)
  YAML.safe_load(text, permitted_classes: [Symbol], aliases: true)
rescue ArgumentError
  YAML.safe_load(text, [Symbol], [], true) # older Psych: positional arguments
end

def perm_violations(perms, where)
  case perms
  when nil then []
  when String
    perms == 'read-all' ? [] : ["#{where}: write permission (permissions: #{perms})"]
  when Hash
    perms.each_with_object([]) do |(scope, level), out|
      next if level.is_a?(String) && LEVELS_OK.include?(level.downcase)

      out << "#{where}: write permission (#{scope}: #{level.inspect})"
    end
  else
    ["#{where}: write permission (unrecognised permissions value)"]
  end
end

def triggers(doc)
  on = doc.key?('on') ? doc['on'] : doc[true] # YAML 1.1 reads bare `on` as true
  case on
  when String then [on]
  when Array then on.map(&:to_s)
  when Hash then on.keys.map(&:to_s)
  else []
  end
end

def each_scalar(node, &block)
  case node
  when Hash then node.each { |k, v| each_scalar(k, &block); each_scalar(v, &block) }
  when Array then node.each { |v| each_scalar(v, &block) }
  when String then block.call(node)
  end
end

def secret_keys?(node)
  case node
  when Hash then node.any? { |k, v| k.to_s == 'secrets' || secret_keys?(v) }
  when Array then node.any? { |v| secret_keys?(v) }
  else false
  end
end

def uses_violation(uses)
  return nil unless uses.is_a?(String)
  return nil if uses.start_with?('./')

  uses.match?(PINNED) ? nil : uses
end

# This reviewed workflow and its executable scripts are the only secret admission.
# Any byte change requires a new explicit review and updated pins.
OPENROUTER_SECRET_FILES = {
  '.github/workflows/wave3-openrouter-live.yml' => '65b348bff6aa1bcbe01cd5f98987d38572dc9fa498ba828adbc47431347fa8c3',
  '.github/scripts/runtime-live-preflight/openrouter.py' => '4558895a08e4aa6a7433c3c72f38597a177ebc1339921acda3b19cfd08088295',
  '.github/scripts/runtime-live-preflight/test_openrouter.py' => '5553d946b5fa2e009f4d0e23a14afed593b7c4e6fe718cf1e270f5e31c310d5f',
  '.github/scripts/wave3-qualification/openrouter_job.py' => '80c3435e0de65ec9cabd6e7a355e052697824b1fc29e04bfc0636f107846080c',
  '.github/scripts/wave3-qualification/test_openrouter_job.py' => '016384296fb4eddc98ae3ccc36a2ad12bb8fd0ac8d8946ecc96f1450241beceb',
}.freeze

def reviewed_openrouter_secret_workflow?(file)
  return false unless File.basename(file) == 'wave3-openrouter-live.yml' &&
                      File.basename(File.dirname(file)) == 'workflows' &&
                      File.basename(File.dirname(File.dirname(file))) == '.github'

  root = File.expand_path('../..', File.dirname(file))
  OPENROUTER_SECRET_FILES.all? do |relative, expected|
    path = File.join(root, relative)
    File.file?(path) && File.realpath(path) == path && Digest::SHA256.file(path).hexdigest == expected
  end
rescue SystemCallError
  false
end

def audit(file)
  out = []
  begin
    doc = parse(File.read(file))
  rescue StandardError => e
    return ["#{file}: unparseable (#{e.class})"]
  end
  return ["#{file}: unparseable (not a mapping)"] unless doc.is_a?(Hash)

  out << "#{file}: no top-level permissions" unless doc.key?('permissions')
  out.concat(perm_violations(doc['permissions'], file))
  triggers(doc).each do |t|
    out << "#{file}: privileged trigger #{t}" if PRIVILEGED.include?(t)
  end
  secret = false
  each_scalar(doc) { |s| secret ||= s.match?(/\$\{\{[^}]*\bsecrets\b/m) }
  secret ||= secret_keys?(doc['jobs'])
  out << "#{file}: secret exposed to job" if secret && !reviewed_openrouter_secret_workflow?(file)

  (doc['jobs'] || {}).each do |name, job|
    next unless job.is_a?(Hash)

    out.concat(perm_violations(job['permissions'], "#{file} job #{name}"))
    if (bad = uses_violation(job['uses']))
      out << "#{file} job #{name}: action not pinned to a full commit SHA (#{bad})"
    end
    (job['steps'] || []).each_with_index do |step, n|
      next unless step.is_a?(Hash)

      if (bad = uses_violation(step['uses']))
        out << "#{file} job #{name} step #{n}: action not pinned to a full commit SHA (#{bad})"
      end
      next unless step['uses'].is_a?(String) && step['uses'].start_with?('actions/checkout@')

      persist = (step['with'] || {})['persist-credentials']
      unless persist == false || persist.to_s == 'false'
        out << "#{file} job #{name} step #{n}: checkout persists credentials"
      end
    end
  end
  out
end

dir = ARGV.fetch(0) { abort 'usage: workflow_policy.rb <dir>' }
files = Dir.glob(File.join(dir, '*.{yml,yaml}')).sort
if files.empty?
  puts "#{dir}: no workflow files"
  exit 1
end
violations = files.flat_map { |f| audit(f) }
puts violations
exit(violations.empty? ? 0 : 1)
