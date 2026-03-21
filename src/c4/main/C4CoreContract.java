package main;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/*
 * Versioned C4 core contract used as architecture boundary between:
 * 1) Java reference lane (C4 source-of-truth)
 * 2) Rust runtime lane (shell adapter / future Rust C4)
 *
 * Keep this intentionally small and stable.
 */
public final class C4CoreContract {
    public static final int VERSION = 1;

    public enum TokenKind {
        IDENT,
        STRING,
        NUMBER,
        TRUE,
        FALSE,
        NULL,
        UNDEFINED,
        LET,
        CONST,
        VAR,
        LPAR,
        RPAR,
        DOT,
        COMMA,
        SEMI,
        ASSIGN,
        EQ,
        STRICT_EQ,
        PLUS,
        MINUS,
        STAR,
        SLASH,
        EOF,
        UNKNOWN
    }

    public enum ExprNodeKind {
        STRING_LITERAL,
        NUMBER_LITERAL,
        BOOLEAN_LITERAL,
        NULL_LITERAL,
        UNDEFINED_LITERAL,
        IDENTIFIER,
        MEMBER_ACCESS,
        CALL,
        BINARY,
        GROUP,
        DECLARATION,
        ASSIGNMENT,
        PROGRAM
    }

    public enum DiagnosticSeverity {
        ERROR,
        WARNING,
        INFO
    }

    public static final class Span {
        public final int start;
        public final int end;

        public Span(int start, int end) {
            this.start = start;
            this.end = end;
        }
    }

    public static final class TokenRef {
        public final TokenKind kind;
        public final Span span;

        public TokenRef(TokenKind kind, Span span) {
            this.kind = kind;
            this.span = span;
        }
    }

    public static final class NodeRef {
        public final ExprNodeKind kind;
        public final Span span;

        public NodeRef(ExprNodeKind kind, Span span) {
            this.kind = kind;
            this.span = span;
        }
    }

    public static final class Diagnostic {
        public final String code;
        public final String message;
        public final DiagnosticSeverity severity;
        public final Span span;

        public Diagnostic(String code, String message, DiagnosticSeverity severity, Span span) {
            this.code = code;
            this.message = message;
            this.severity = severity;
            this.span = span;
        }
    }

    public static final class AnalysisSchemaV1 {
        public final int version;
        public final List<TokenRef> tokens;
        public final List<NodeRef> nodes;
        public final List<Diagnostic> diagnostics;

        public AnalysisSchemaV1(
            int version,
            List<TokenRef> tokens,
            List<NodeRef> nodes,
            List<Diagnostic> diagnostics
        ) {
            this.version = version;
            this.tokens = Collections.unmodifiableList(new ArrayList<TokenRef>(tokens));
            this.nodes = Collections.unmodifiableList(new ArrayList<NodeRef>(nodes));
            this.diagnostics = Collections.unmodifiableList(new ArrayList<Diagnostic>(diagnostics));
        }

        public static AnalysisSchemaV1 empty() {
            return new AnalysisSchemaV1(
                VERSION,
                Collections.<TokenRef>emptyList(),
                Collections.<NodeRef>emptyList(),
                Collections.<Diagnostic>emptyList()
            );
        }
    }

    public enum ResultHint {
        STRING,
        NUMBER,
        BOOLEAN,
        NULL,
        UNDEFINED,
        FUNCTION,
        OBJECT,
        UNKNOWN
    }

    public enum SymbolRole {
        DECL,
        ASSIGN,
        READ,
        CALL
    }

    private C4CoreContract() {
    }
}
