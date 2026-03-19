package main;

import java.io.*;
import code.*;

import lexer.*;
import parser.*;
import inter.Program;
import treewalker.*;


public class Main {

	/*
	 * Ein Parser für die Beispielsprache aus dem Buch von Aho, Lem, Sethi und
	 * Ullmann
	 * 
	 * Die main-Methode erzeugt zunächst eine Instanz eines lexikalen Scanners,
	 * der dann zum Erzeugen einer Instanz eines Parsers benutzt wird. Dieser
	 * Parser liest die Eingabe von der Console und wirft im Fall eines
	 * Syntaxfehlers eine IOException.
	 * 
	 * Parallel dazu wird ein Syntaxbaum mit Wurzel root erzeugt.
	 * Danach wird durch den Treewalker ltw der Syntaxbaum ausgegeben.
	 * Anschließend wird eine semntische Anaalyse und eine Transformation der 
	 * Feldzugriffe durchgeführt und das Ergenis jeweils ausgegeben
	 * 
	 * Abschließend wird der Syntaxbaum durch einen CodeGenWalker in 
	 * Drei-Adress-Code übersetzt. 
	 * 
	 */

	public static void main(String[] args) throws IOException {
		StringBuffer b = new StringBuffer();
		int maxInputLength = 1500;
		
		char[] iProg = new char[maxInputLength];

		System.out.println("Eingabe-Datei:");		
		try {
				char dat = (char)System.in.read();
				while (dat != '\n' && dat != '\r') {	
						b.append(dat);
						dat = (char)System.in.read();
				} 			
			String fname = b.toString();
			FileReader fileReader = new FileReader(fname);
			int charIn = fileReader.read(iProg);
			if (charIn == maxInputLength) {
				System.out.print("Input Program is too long");
			}
			fileReader.close();
			
		} catch (IOException e) {
			e.printStackTrace();
		}
		
		Lexer lex = new Lexer(iProg);
		Parser parse = new Parser(lex);
		Program root = parse.program();
		System.out.println("\nParsing erfolgreich beendet\n");		
		System.out.println("\nSyntaxbaum:\n");
		LinTreeWalker ltw = new LinTreeWalker();
		ltw.walk(root, "");
		
		System.out.println("\nSemantische Analyse:");		
		SemanticWalker smw = new SemanticWalker();
		smw.walk(root, null);
		System.out.println("und der neue Syntaxbaum:\n");

		LinTreeWalkerType ltwt = new LinTreeWalkerType();
		ltwt.walk(root, "");
		
		System.out.println("\nTranformation des Arrayzugriffe:");

		TransformWalker tfw = new TransformWalker();
		tfw.walk(root, null);
		System.out.println("modifizierter Syntaxbaum:\n");

		ltwt.walk(root, "");
		
		System.out.println("\nDrei-Adress-Code Erzeugung:\n");
		
		CodeGenWalker cgw = new CodeGenWalker();
		cgw.walk(root, null); 
		
		//Ausgabe des Ergebnisses
		
		int i = 1;
		for (DreiAdrCode c : cgw.code)
				System.out.println(i++ +":\t" + c.toString());

		
		System.out.println("\nCode-Erzeugung beendet");

		System.out.println("\nUebersetzung beendet");

	}


}
