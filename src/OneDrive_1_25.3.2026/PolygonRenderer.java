package com.mycompany.SwingOpenGL;

import com.jogamp.opengl.GL2;
import com.mycompany.SwingOpenGL.Polygon.Vertex;

public class PolygonRenderer {
    public static void renderPolygon(GL2 gl2, Polygon p){
        renderVertices(gl2, p);
        
        //renderStarPoint(gl2, p);
        
        renderEdges(gl2, p);
        
        if (p.convexHull != null)
           renderConvexHull(gl2, p);
    }
    
     // to render the convex hull we can rely on p.convexHull
     public static void renderConvexHull(GL2 gl2, Polygon p){
        Vertex next = p.convexHull.startVertex;
        gl2.glColor3d(0, 0, 1);
        gl2.glBegin (GL2.GL_LINE_LOOP); 
        do{
            gl2.glVertex2d(next.x, next.y);
            next = next.nextVertex;
        }while(p.convexHull.startVertex != next);
        gl2.glEnd();            
        gl2.glColor3d(0, 0, 0);
    }
     

    
    public static void renderVertices(GL2 gl2, Polygon p){
        Vertex next = p.startVertex;
        gl2.glBegin (GL2.GL_POINTS); // gl2.glBegin (GL2.GL_POLYGON);
        do{
            gl2.glVertex2d(next.x, next.y);
            next = next.nextVertex;
        }while(p.startVertex != next);
        gl2.glEnd();
    }
    
    public static void renderEdges(GL2 gl2, Polygon p){
        Vertex next = p.startVertex;
        gl2.glBegin (GL2.GL_LINES); 
        do{
            gl2.glVertex2d(next.x, next.y);
            gl2.glVertex2d(next.nextVertex.x, next.nextVertex.y);
            next = next.nextVertex;
        }while(p.startVertex != next);
        gl2.glEnd();    
    }
    
    // Graham-Scan-Algorithmus
    public static void renderStarPoint(GL2 gl2, Polygon p){
        Vertex next = p.startVertex;
        gl2.glBegin (GL2.GL_LINES); 
        do{
            gl2.glVertex2d(p.starPoint.x, p.starPoint.y);
            gl2.glVertex2d(next.x, next.y);
            next = next.nextVertex;
        }while(p.startVertex != next);
        gl2.glEnd();         
    }
}
