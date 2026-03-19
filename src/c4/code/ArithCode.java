package code;


import lexer.Token;
import inter.Id;

/*
 * Diese Klasse ist die abstrakte Klasse aller einfachen arithmetischen Drei-Adress-Befehle
 * Jeder dieser Befehle hat einen Identifier auf der linken Seite und 
 * höchstens einen Operator auf der rechten Seite
 */

public abstract class ArithCode extends DreiAdrCode {
	Id leftSide;
	Token operator;

}
