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
  out << "#{file}: secret exposed to job" if secret

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
